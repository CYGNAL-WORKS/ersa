//! Admission (ARCH-CORE, Admission; ADR 0037): the one place that decides
//! every change to which projects, workspaces and sessions exist and to what
//! is under way on each.
//!
//! There are two layers. [`step`] is the transition function: plain data
//! in, plain data out, no waiting and no I/O, so a test can feed it any
//! ordering directly. [`Admission`] puts one lock around it and hands out
//! [`UnderWay`] handles, so the rest of Core can use it from many tasks.
//!
//! An input is a command (a request asking to do something) or a report (an
//! operation saying what happened). A command is admitted, refused, or held
//! until a report changes what it waits for. Slow work never happens here:
//! an operation is admitted, does its work outside, writes the store, and
//! then reports.
//!
//! Every `match` over a phase below names every case (ARCH-CORE-R15); the
//! lint on the next line makes a catch-all arm a build failure.
#![deny(clippy::wildcard_enum_match_arm)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::oneshot;

/// The id of a project, workspace or session, as the store holds it.
pub type Id = String;

/// Where a project's folder is: its host, and its path as that host
/// compares paths. One folder is one project.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Folder {
    pub host: String,
    pub key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProjectPhase {
    /// Admitted, its rows not yet stored.
    Adding,
    Present,
    /// A `project.remove` is under way; its root workspace is held.
    Removing,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectEntry {
    pub folder: Folder,
    pub phase: ProjectPhase,
}

/// The state a removal took its workspace from, and puts back when refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Replaced {
    Ready,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspacePhase {
    Creating,
    Ready,
    Error,
    /// A removal holds it.
    Removing {
        replaced: Replaced,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WorkspaceEntry {
    pub project: Id,
    /// Whether it is the project's own folder.
    pub root: bool,
    pub phase: WorkspacePhase,
    /// The session whose `session.delete` holds the workspace.
    pub deleting: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionEntry {
    pub workspace: Id,
    /// Found in the backend's records rather than started by Ersa: it never
    /// holds its workspace back, and goes with it.
    pub found: bool,
    /// Whether its row is stored yet.
    pub stored: bool,
    /// Its one exclusive operation, if one is under way: a session has
    /// at most one, and another asked for meanwhile waits.
    pub exclusive: Option<Exclusive>,
}

/// A session's exclusive operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Exclusive {
    /// A `session.remove`: when it ends well, the session is gone.
    Removal,
    /// Any other: a stop, a start, a view being opened, an archive, an
    /// arrangement, a delete's mark.
    Other,
}

/// Everything admission decides from. Ordered maps and no counters, so two
/// states reached by different routes compare equal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct State {
    projects: BTreeMap<Id, ProjectEntry>,
    workspaces: BTreeMap<Id, WorkspaceEntry>,
    sessions: BTreeMap<Id, SessionEntry>,
    /// Commands waiting for a report, in the order they arrived.
    held: Vec<Command>,
}

/// What the store holds when Core starts.
#[derive(Clone, Debug, Default)]
pub struct Existing {
    pub projects: Vec<(Id, Folder)>,
    pub workspaces: Vec<ExistingWorkspace>,
    /// Each session's id, its workspace's id, and whether it was found in
    /// the backend's records.
    pub sessions: Vec<(Id, Id, bool)>,
}

#[derive(Clone, Debug)]
pub struct ExistingWorkspace {
    pub id: Id,
    pub project: Id,
    pub root: bool,
    pub phase: WorkspacePhase,
}

impl State {
    /// The state for what the store holds: nothing is under way.
    pub fn restored(existing: Existing) -> State {
        let mut state = State::default();
        for (id, folder) in existing.projects {
            let phase = ProjectPhase::Present;
            state.projects.insert(id, ProjectEntry { folder, phase });
        }
        for workspace in existing.workspaces {
            let entry = WorkspaceEntry {
                project: workspace.project,
                root: workspace.root,
                phase: workspace.phase,
                deleting: None,
            };
            state.workspaces.insert(workspace.id, entry);
        }
        for (id, workspace, found) in existing.sessions {
            let stored = true;
            state.sessions.insert(
                id,
                SessionEntry {
                    workspace,
                    found,
                    stored,
                    exclusive: None,
                },
            );
        }
        state
    }

    pub fn project(&self, id: &str) -> Option<&ProjectEntry> {
        self.projects.get(id)
    }

    pub fn workspace(&self, id: &str) -> Option<&WorkspaceEntry> {
        self.workspaces.get(id)
    }

    pub fn session(&self, id: &str) -> Option<&SessionEntry> {
        self.sessions.get(id)
    }

    pub fn projects(&self) -> impl Iterator<Item = (&Id, &ProjectEntry)> {
        self.projects.iter()
    }

    pub fn workspaces(&self) -> impl Iterator<Item = (&Id, &WorkspaceEntry)> {
        self.workspaces.iter()
    }

    pub fn sessions(&self) -> impl Iterator<Item = (&Id, &SessionEntry)> {
        self.sessions.iter()
    }

    /// The commands waiting for a report, in the order they arrived.
    pub fn held(&self) -> &[Command] {
        &self.held
    }

    /// Whether a session Ersa started is in `workspace`, stored or not.
    fn has_started_session(&self, workspace: &str) -> bool {
        self.sessions
            .values()
            .any(|session| session.workspace == workspace && !session.found)
    }

    /// Whether a session was admitted into `workspace` and its row is not
    /// stored yet. A removal or a delete waits for it: which rows there are
    /// to take, or to count, is not known until that reports.
    fn has_arriving_session(&self, workspace: &str) -> bool {
        self.sessions
            .values()
            .any(|session| session.workspace == workspace && !session.stored)
    }

    /// Whether something holds `workspace`: its own removal, its project's
    /// removal when it is the project's own folder, or a session's delete.
    /// A held workspace takes no session, and no other removal or delete.
    /// One admission does not have counts as held: it went with a removal
    /// or a delete, and nothing may start in what is left of it.
    pub fn workspace_held(&self, workspace: &str) -> bool {
        let Some(entry) = self.workspaces.get(workspace) else {
            return true;
        };
        let removing = match entry.phase {
            WorkspacePhase::Removing { .. } => true,
            WorkspacePhase::Creating | WorkspacePhase::Ready | WorkspacePhase::Error => false,
        };
        let project_going = entry.root
            && match self.projects.get(&entry.project).map(|entry| entry.phase) {
                Some(ProjectPhase::Removing) => true,
                Some(ProjectPhase::Adding | ProjectPhase::Present) | None => false,
            };
        removing || project_going || entry.deleting.is_some()
    }

    fn has_session(&self, workspace: &str) -> bool {
        self.sessions
            .values()
            .any(|session| session.workspace == workspace)
    }

    /// The project's root workspace.
    fn root_of(&self, project: &str) -> Option<&Id> {
        self.workspaces
            .iter()
            .find(|(_, workspace)| workspace.project == project && workspace.root)
            .map(|(id, _)| id)
    }

    /// Forgets `project`, its workspaces and every session in them.
    fn drop_project(&mut self, project: &str) {
        self.projects.remove(project);
        let workspaces: Vec<Id> = self
            .workspaces
            .iter()
            .filter(|(_, workspace)| workspace.project == project)
            .map(|(id, _)| id.clone())
            .collect();
        for workspace in workspaces {
            self.drop_workspace(&workspace);
        }
    }

    /// Forgets `workspace` and every session in it.
    fn drop_workspace(&mut self, workspace: &str) {
        self.workspaces.remove(workspace);
        self.sessions
            .retain(|_, session| session.workspace != workspace);
    }
}

/// A request asking admission to let an operation begin.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// `root` is the id its root workspace will have, so that a request
    /// naming either waits for the add to report rather than be told there
    /// is no such thing.
    AddProject {
        project: Id,
        root: Id,
        folder: Folder,
    },
    RemoveProject {
        project: Id,
    },
    CreateWorkspace {
        project: Id,
        workspace: Id,
    },
    RemoveWorkspace {
        workspace: Id,
    },
    /// A new session in `workspace`: one Ersa starts, or one `found` in the
    /// backend's records.
    CreateSession {
        workspace: Id,
        session: Id,
        found: bool,
    },
    /// `session.delete`: holds the session's workspace until it ends.
    DeleteSession {
        session: Id,
    },
    /// `session.remove`: the session's exclusive operation while it runs.
    RemoveSession {
        session: Id,
    },
    /// Any other exclusive operation on the session: admitted when none is
    /// under way on it, held until then.
    BeginExclusive {
        session: Id,
    },
}

/// An operation admission has let begin, named by what it is on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    AddProject(Id),
    RemoveProject(Id),
    CreateWorkspace(Id),
    RemoveWorkspace(Id),
    CreateSession(Id),
    DeleteSession(Id),
    RemoveSession(Id),
    Exclusive(Id),
}

/// What happened, said once it is stored. A report is never refused or
/// held (ARCH-CORE-R17); one that does not fit the state changes nothing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Report {
    /// The project and its root workspace are stored.
    ProjectStored {
        project: Id,
    },
    /// The project's rows are gone, with its root workspace's and those of
    /// the sessions found there.
    ProjectRemoved {
        project: Id,
    },
    /// The removal did not happen; the project is as it was.
    ProjectRemovalRefused {
        project: Id,
    },
    WorkspaceReady {
        workspace: Id,
    },
    /// Making it failed: its row is `kept` as `error`, or gone.
    WorkspaceCreateFailed {
        workspace: Id,
        kept: bool,
    },
    /// The workspace's row is gone, with those of the sessions found there.
    WorkspaceRemoved {
        workspace: Id,
    },
    /// The removal did not happen; the workspace is in the state it had.
    WorkspaceRemovalRefused {
        workspace: Id,
    },
    /// The removal failed part-way; the workspace is in `error`.
    WorkspaceRemovalFailed {
        workspace: Id,
    },
    /// A `session.delete` that takes the workspace's files marked it `error`.
    WorkspaceErrored {
        workspace: Id,
    },
    /// A finished `session.delete` removed the workspace's row.
    WorkspaceDeleted {
        workspace: Id,
    },
    SessionStored {
        session: Id,
    },
    /// The session's row is gone.
    SessionGone {
        session: Id,
    },
    /// The operation's task ended without saying how (ARCH-CORE-R16).
    Ended(Operation),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    Command(Command),
    Report(Report),
}

/// Why a command was refused. The API layer gives each its message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// The folder is the project `existing`.
    AlreadyProject {
        existing: Id,
    },
    NoProject,
    /// Another removal of the project is under way; `root` is its root
    /// workspace, which that removal holds.
    ProjectBusy {
        root: Option<Id>,
    },
    /// A session Ersa started is in its folder. (One found in the backend's
    /// records that Core runs refuses a removal too, after admission: the
    /// run phase is not admission's yet.)
    ProjectHasSessions,
    ProjectHasWorkspaces,
    NoWorkspace,
    /// The id is taken.
    WorkspaceExists,
    /// The project's own folder goes only with the project.
    RootWorkspace,
    /// It is being made or removed.
    WorkspaceBusy,
    WorkspaceHasSessions,
    WorkspaceNotReady,
    /// A removal or a delete holds the workspace.
    WorkspaceHeld,
    /// The id is taken.
    SessionExists,
    NoSession,
}

/// Admission's answer to one input.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// A command: the operation may begin, and is recorded.
    Admitted(Operation),
    /// A command: nothing changed.
    Refused(Refusal),
    /// A command: kept, and decided again after each report.
    Held,
    /// A report: taken.
    Noted,
    /// A report that does not fit the state: nothing changed.
    Stale,
}

/// What one input did.
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    pub outcome: Outcome,
    /// The held commands this input answered: each one's place among
    /// [`State::held`] before this step, ascending, with its answer. They
    /// are held no longer.
    pub released: Vec<(usize, Outcome)>,
}

/// The transition function (ARCH-CORE-R14): applies `input` to `state` and
/// says what it did. It never waits and performs no I/O.
pub fn step(state: &mut State, input: Input) -> Step {
    match input {
        Input::Command(command) => {
            let outcome = decide(state, &command);
            // An admitted command changed the state, which may answer a
            // held one: a session admitted into a workspace refuses the
            // removal waiting there.
            let released = match outcome {
                Outcome::Admitted(_) => release(state),
                Outcome::Held => {
                    state.held.push(command);
                    Vec::new()
                }
                Outcome::Refused(_) | Outcome::Noted | Outcome::Stale => Vec::new(),
            };
            Step { outcome, released }
        }
        Input::Report(report) => {
            let outcome = note(state, report);
            // A report that changed something may be what a held command
            // waits for.
            let released = match outcome {
                Outcome::Noted => release(state),
                Outcome::Stale | Outcome::Admitted(_) | Outcome::Refused(_) | Outcome::Held => {
                    Vec::new()
                }
            };
            Step { outcome, released }
        }
    }
}

/// Decides every held command again, in the order they arrived, and again
/// until a pass answers none: admitting one may answer another that came
/// before it. Each answered command leaves `held`, so this ends.
fn release(state: &mut State) -> Vec<(usize, Outcome)> {
    let mut waiting: Vec<(usize, Command)> = std::mem::take(&mut state.held)
        .into_iter()
        .enumerate()
        .collect();
    let mut released = Vec::new();
    loop {
        let mut still = Vec::new();
        let before = released.len();
        for (place, command) in waiting {
            match decide(state, &command) {
                Outcome::Held => still.push((place, command)),
                answer @ (Outcome::Admitted(_)
                | Outcome::Refused(_)
                | Outcome::Noted
                | Outcome::Stale) => released.push((place, answer)),
            }
        }
        waiting = still;
        if released.len() == before {
            break;
        }
    }
    state.held = waiting.into_iter().map(|(_, command)| command).collect();
    released.sort_by_key(|(place, _)| *place);
    released
}

/// The table for commands. An admitted command is recorded in `state`; a
/// refused or held one leaves it as it was.
fn decide(state: &mut State, command: &Command) -> Outcome {
    match command {
        Command::AddProject {
            project,
            root,
            folder,
        } => {
            if state.projects.contains_key(project) {
                let existing = project.clone();
                return Outcome::Refused(Refusal::AlreadyProject { existing });
            }
            if state.workspaces.contains_key(root) {
                return Outcome::Refused(Refusal::WorkspaceExists);
            }
            let at_folder = state
                .projects
                .iter()
                .find(|(_, entry)| &entry.folder == folder)
                .map(|(id, entry)| (id.clone(), entry.phase));
            match at_folder {
                Some((existing, ProjectPhase::Present)) => {
                    Outcome::Refused(Refusal::AlreadyProject { existing })
                }
                // Whether the folder is a project is not known until that
                // add or removal reports.
                Some((_, ProjectPhase::Adding | ProjectPhase::Removing)) => Outcome::Held,
                None => {
                    let entry = ProjectEntry {
                        folder: folder.clone(),
                        phase: ProjectPhase::Adding,
                    };
                    state.projects.insert(project.clone(), entry);
                    let entry = WorkspaceEntry {
                        project: project.clone(),
                        root: true,
                        phase: WorkspacePhase::Ready,
                        deleting: None,
                    };
                    state.workspaces.insert(root.clone(), entry);
                    Outcome::Admitted(Operation::AddProject(project.clone()))
                }
            }
        }
        Command::RemoveProject { project } => {
            let root = state.root_of(project).cloned();
            match state.projects.get(project).map(|entry| entry.phase) {
                None => Outcome::Refused(Refusal::NoProject),
                // Stored, perhaps, and its add yet to report: whether there
                // is a project to remove is known once it does.
                Some(ProjectPhase::Adding) => Outcome::Held,
                Some(ProjectPhase::Removing) => Outcome::Refused(Refusal::ProjectBusy { root }),
                Some(ProjectPhase::Present) => {
                    let sessions = root
                        .as_ref()
                        .is_some_and(|root| state.has_started_session(root));
                    let isolated = state
                        .workspaces
                        .values()
                        .any(|workspace| &workspace.project == project && !workspace.root);
                    let arriving = root
                        .as_ref()
                        .is_some_and(|root| state.has_arriving_session(root));
                    let deleting = root
                        .as_ref()
                        .and_then(|root| state.workspaces.get(root))
                        .is_some_and(|root| root.deleting.is_some());
                    if sessions {
                        Outcome::Refused(Refusal::ProjectHasSessions)
                    } else if isolated {
                        Outcome::Refused(Refusal::ProjectHasWorkspaces)
                    } else if deleting {
                        // A delete of a session found there holds the folder.
                        Outcome::Refused(Refusal::ProjectBusy { root })
                    } else if arriving {
                        Outcome::Held
                    } else {
                        if let Some(entry) = state.projects.get_mut(project) {
                            entry.phase = ProjectPhase::Removing;
                        }
                        Outcome::Admitted(Operation::RemoveProject(project.clone()))
                    }
                }
            }
        }
        Command::CreateWorkspace { project, workspace } => {
            if state.workspaces.contains_key(workspace) {
                return Outcome::Refused(Refusal::WorkspaceExists);
            }
            match state.projects.get(project).map(|entry| entry.phase) {
                None => Outcome::Refused(Refusal::NoProject),
                // Whether there is a project to make it in is not known
                // until the add or the removal reports.
                Some(ProjectPhase::Adding | ProjectPhase::Removing) => Outcome::Held,
                Some(ProjectPhase::Present) => {
                    let entry = WorkspaceEntry {
                        project: project.clone(),
                        root: false,
                        phase: WorkspacePhase::Creating,
                        deleting: None,
                    };
                    state.workspaces.insert(workspace.clone(), entry);
                    Outcome::Admitted(Operation::CreateWorkspace(workspace.clone()))
                }
            }
        }
        Command::RemoveWorkspace { workspace } => {
            let Some(entry) = state.workspaces.get(workspace) else {
                return Outcome::Refused(Refusal::NoWorkspace);
            };
            if entry.root {
                return Outcome::Refused(Refusal::RootWorkspace);
            }
            let replaced = match entry.phase {
                WorkspacePhase::Creating | WorkspacePhase::Removing { .. } => {
                    return Outcome::Refused(Refusal::WorkspaceBusy);
                }
                WorkspacePhase::Ready => Replaced::Ready,
                WorkspacePhase::Error => Replaced::Error,
            };
            if state.has_started_session(workspace) {
                return Outcome::Refused(Refusal::WorkspaceHasSessions);
            }
            // A delete of a session found there holds it.
            if entry.deleting.is_some() {
                return Outcome::Refused(Refusal::WorkspaceHeld);
            }
            if state.has_arriving_session(workspace) {
                return Outcome::Held;
            }
            if let Some(entry) = state.workspaces.get_mut(workspace) {
                entry.phase = WorkspacePhase::Removing { replaced };
            }
            Outcome::Admitted(Operation::RemoveWorkspace(workspace.clone()))
        }
        Command::CreateSession {
            workspace,
            session,
            found,
        } => {
            if state.sessions.contains_key(session) {
                return Outcome::Refused(Refusal::SessionExists);
            }
            let Some(entry) = state.workspaces.get(workspace) else {
                return Outcome::Refused(Refusal::NoWorkspace);
            };
            match entry.phase {
                WorkspacePhase::Creating
                | WorkspacePhase::Error
                | WorkspacePhase::Removing { .. } => {
                    return Outcome::Refused(Refusal::WorkspaceNotReady);
                }
                WorkspacePhase::Ready => {}
            }
            // A delete holds it: what the delete counted must not change.
            if entry.deleting.is_some() {
                return Outcome::Refused(Refusal::WorkspaceHeld);
            }
            match state.projects.get(&entry.project).map(|entry| entry.phase) {
                Some(ProjectPhase::Present) => {}
                // The folder is on its way out: nothing joins it, a session
                // found in the backend's records no more than one Ersa starts.
                Some(ProjectPhase::Removing) => {
                    return Outcome::Refused(Refusal::WorkspaceHeld);
                }
                // Its project's add is yet to report.
                Some(ProjectPhase::Adding) => return Outcome::Held,
                // A workspace always has a project.
                None => return Outcome::Refused(Refusal::NoWorkspace),
            }
            let entry = SessionEntry {
                workspace: workspace.clone(),
                found: *found,
                stored: false,
                exclusive: None,
            };
            state.sessions.insert(session.clone(), entry);
            Outcome::Admitted(Operation::CreateSession(session.clone()))
        }
        Command::DeleteSession { session } => {
            let Some(entry) = state.sessions.get(session) else {
                return Outcome::Refused(Refusal::NoSession);
            };
            let workspace = entry.workspace.clone();
            if state.workspace_held(&workspace) {
                return Outcome::Refused(Refusal::WorkspaceHeld);
            }
            // Whether there is a session to delete is not known until its
            // removal reports; nor what the delete would count, until every
            // session admitted into the workspace is stored.
            if entry.exclusive == Some(Exclusive::Removal) || state.has_arriving_session(&workspace)
            {
                return Outcome::Held;
            }
            if let Some(entry) = state.workspaces.get_mut(&workspace) {
                entry.deleting = Some(session.clone());
            }
            Outcome::Admitted(Operation::DeleteSession(session.clone()))
        }
        Command::RemoveSession { session } => {
            let Some(entry) = state.sessions.get(session) else {
                return Outcome::Refused(Refusal::NoSession);
            };
            if state.workspace_held(&entry.workspace) {
                return Outcome::Refused(Refusal::WorkspaceHeld);
            }
            // Its row is still being stored, or an exclusive operation is
            // under way on it, another removal among them: whether there is
            // a session to remove is known once that ends.
            if !entry.stored || entry.exclusive.is_some() {
                return Outcome::Held;
            }
            if let Some(entry) = state.sessions.get_mut(session) {
                entry.exclusive = Some(Exclusive::Removal);
            }
            Outcome::Admitted(Operation::RemoveSession(session.clone()))
        }
        Command::BeginExclusive { session } => {
            let Some(entry) = state.sessions.get_mut(session) else {
                return Outcome::Refused(Refusal::NoSession);
            };
            // One at a time: the next waits for the one under way to end.
            // So does one asked of a session whose row is being stored.
            if !entry.stored || entry.exclusive.is_some() {
                return Outcome::Held;
            }
            entry.exclusive = Some(Exclusive::Other);
            Outcome::Admitted(Operation::Exclusive(session.clone()))
        }
    }
}

/// The table for reports: `Noted` when the report changed `state`, `Stale`
/// when it does not fit it.
fn note(state: &mut State, report: Report) -> Outcome {
    match report {
        Report::ProjectStored { project } => {
            match state
                .projects
                .get_mut(&project)
                .map(|entry| &mut entry.phase)
            {
                Some(phase @ ProjectPhase::Adding) => *phase = ProjectPhase::Present,
                None | Some(ProjectPhase::Present | ProjectPhase::Removing) => {
                    return Outcome::Stale;
                }
            }
            Outcome::Noted
        }
        Report::ProjectRemoved { project } => {
            match state.projects.get(&project).map(|entry| entry.phase) {
                Some(ProjectPhase::Removing) => {}
                None | Some(ProjectPhase::Adding | ProjectPhase::Present) => {
                    return Outcome::Stale;
                }
            }
            state.drop_project(&project);
            Outcome::Noted
        }
        Report::ProjectRemovalRefused { project } => put_project_back(state, &project),
        Report::WorkspaceReady { workspace } => {
            set_workspace(state, &workspace, |phase| match phase {
                WorkspacePhase::Creating => Some(WorkspacePhase::Ready),
                WorkspacePhase::Ready | WorkspacePhase::Error | WorkspacePhase::Removing { .. } => {
                    None
                }
            })
        }
        Report::WorkspaceCreateFailed { workspace, kept } => {
            match state.workspaces.get(&workspace).map(|entry| entry.phase) {
                Some(WorkspacePhase::Creating) => {}
                None
                | Some(
                    WorkspacePhase::Ready | WorkspacePhase::Error | WorkspacePhase::Removing { .. },
                ) => return Outcome::Stale,
            }
            if kept {
                set_workspace(state, &workspace, |_| Some(WorkspacePhase::Error))
            } else {
                state.drop_workspace(&workspace);
                Outcome::Noted
            }
        }
        Report::WorkspaceRemoved { workspace } => {
            match state.workspaces.get(&workspace).map(|entry| entry.phase) {
                Some(WorkspacePhase::Removing { .. }) => {}
                None
                | Some(WorkspacePhase::Creating | WorkspacePhase::Ready | WorkspacePhase::Error) => {
                    return Outcome::Stale;
                }
            }
            state.drop_workspace(&workspace);
            Outcome::Noted
        }
        Report::WorkspaceRemovalRefused { workspace } => {
            set_workspace(state, &workspace, |phase| match phase {
                WorkspacePhase::Removing {
                    replaced: Replaced::Ready,
                } => Some(WorkspacePhase::Ready),
                WorkspacePhase::Removing {
                    replaced: Replaced::Error,
                } => Some(WorkspacePhase::Error),
                WorkspacePhase::Creating | WorkspacePhase::Ready | WorkspacePhase::Error => None,
            })
        }
        Report::WorkspaceRemovalFailed { workspace } => fail_removal(state, &workspace),
        Report::WorkspaceErrored { workspace } => {
            set_workspace(state, &workspace, |phase| match phase {
                WorkspacePhase::Creating | WorkspacePhase::Ready => Some(WorkspacePhase::Error),
                // The removal keeps its hold; what it puts back when
                // refused is now `error` (PROTO-CORE-API-R94).
                WorkspacePhase::Removing {
                    replaced: Replaced::Ready,
                } => Some(WorkspacePhase::Removing {
                    replaced: Replaced::Error,
                }),
                WorkspacePhase::Error
                | WorkspacePhase::Removing {
                    replaced: Replaced::Error,
                } => None,
            })
        }
        Report::WorkspaceDeleted { workspace } => {
            let Some(entry) = state.workspaces.get(&workspace) else {
                return Outcome::Stale;
            };
            // The store removes the row only when no session refers to it,
            // and never a project's own folder's.
            if entry.root || state.has_session(&workspace) {
                return Outcome::Stale;
            }
            match entry.phase {
                // A workspace still being made has had no session to delete.
                WorkspacePhase::Creating => return Outcome::Stale,
                WorkspacePhase::Ready | WorkspacePhase::Error | WorkspacePhase::Removing { .. } => {
                }
            }
            state.drop_workspace(&workspace);
            Outcome::Noted
        }
        Report::SessionStored { session } => match state.sessions.get_mut(&session) {
            Some(entry) if !entry.stored => {
                entry.stored = true;
                Outcome::Noted
            }
            Some(_) | None => Outcome::Stale,
        },
        Report::SessionGone { session } => match state.sessions.remove(&session) {
            Some(_) => Outcome::Noted,
            None => Outcome::Stale,
        },
        // How the operation ended is unknown, so what it was on goes to the
        // state that claims nothing: a reservation is dropped, a workspace
        // is in `error`, a project is still there.
        Report::Ended(operation) => match operation {
            Operation::AddProject(project) => {
                match state.projects.get(&project).map(|entry| entry.phase) {
                    Some(ProjectPhase::Adding) => {
                        state.drop_project(&project);
                        Outcome::Noted
                    }
                    None | Some(ProjectPhase::Present | ProjectPhase::Removing) => Outcome::Stale,
                }
            }
            Operation::RemoveProject(project) => put_project_back(state, &project),
            Operation::CreateWorkspace(workspace) => {
                set_workspace(state, &workspace, |phase| match phase {
                    WorkspacePhase::Creating => Some(WorkspacePhase::Error),
                    WorkspacePhase::Ready
                    | WorkspacePhase::Error
                    | WorkspacePhase::Removing { .. } => None,
                })
            }
            Operation::RemoveWorkspace(workspace) => fail_removal(state, &workspace),
            Operation::CreateSession(session) => {
                match state.sessions.get(&session).map(|entry| entry.stored) {
                    Some(false) => {
                        state.sessions.remove(&session);
                        Outcome::Noted
                    }
                    Some(true) | None => Outcome::Stale,
                }
            }
            // A delete ends by letting go: what it did, it reported.
            Operation::DeleteSession(session) => {
                let held = state
                    .workspaces
                    .values_mut()
                    .find(|workspace| workspace.deleting.as_ref() == Some(&session));
                match held {
                    Some(workspace) => {
                        workspace.deleting = None;
                        Outcome::Noted
                    }
                    None => Outcome::Stale,
                }
            }
            Operation::RemoveSession(session) => end_exclusive(state, &session, Exclusive::Removal),
            Operation::Exclusive(session) => end_exclusive(state, &session, Exclusive::Other),
        },
    }
}

/// Moves `workspace` to the phase `next` gives for the one it is in;
/// `Stale` when there is no such workspace or `next` gives none.
fn set_workspace(
    state: &mut State,
    workspace: &str,
    next: impl FnOnce(WorkspacePhase) -> Option<WorkspacePhase>,
) -> Outcome {
    let Some(entry) = state.workspaces.get_mut(workspace) else {
        return Outcome::Stale;
    };
    match next(entry.phase) {
        Some(phase) => {
            entry.phase = phase;
            Outcome::Noted
        }
        None => Outcome::Stale,
    }
}

/// Ends the session's exclusive operation, when it is `which`.
fn end_exclusive(state: &mut State, session: &str, which: Exclusive) -> Outcome {
    match state.sessions.get_mut(session) {
        Some(entry) if entry.exclusive == Some(which) => {
            entry.exclusive = None;
            Outcome::Noted
        }
        Some(_) | None => Outcome::Stale,
    }
}

/// A removal that did not end in a refusal leaves its workspace in `error`.
fn fail_removal(state: &mut State, workspace: &str) -> Outcome {
    set_workspace(state, workspace, |phase| match phase {
        WorkspacePhase::Removing { .. } => Some(WorkspacePhase::Error),
        WorkspacePhase::Creating | WorkspacePhase::Ready | WorkspacePhase::Error => None,
    })
}

/// A project whose removal did not happen is there as before.
fn put_project_back(state: &mut State, project: &str) -> Outcome {
    match state
        .projects
        .get_mut(project)
        .map(|entry| &mut entry.phase)
    {
        Some(phase @ ProjectPhase::Removing) => {
            *phase = ProjectPhase::Present;
            Outcome::Noted
        }
        None | Some(ProjectPhase::Adding | ProjectPhase::Present) => Outcome::Stale,
    }
}

// -- The lock around the function, and operations under way ----------------

/// Admission for a running Core: [`step`] behind one lock. The lock is a
/// leaf: it is held only for one step, never across an await and never
/// while taking another lock.
#[derive(Default)]
pub struct Admission {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    state: State,
    /// Who waits for each held command, in the order of [`State::held`].
    waiting: Vec<oneshot::Sender<Outcome>>,
}

impl Admission {
    /// Replaces the state with what the store holds, at startup, before any
    /// request.
    pub fn restore(&self, existing: Existing) {
        let mut inner = self.lock();
        inner.state = State::restored(existing);
        inner.waiting.clear();
    }

    /// Asks to begin an operation. Answers at once unless the command is
    /// held, and then when a report lets it be decided. The [`UnderWay`] of an
    /// admitted operation reports its end, however its task ends.
    pub async fn command(self: &Arc<Self>, command: Command) -> Result<UnderWay, Refusal> {
        let (outcome, waiter) = self.apply(Input::Command(command));
        let outcome = match waiter {
            // Waited for outside the lock. The sender lives in `self`,
            // which this call keeps alive, so it is never dropped unsent.
            Some(waiter) => waiter.await.expect("a held command is always answered"),
            None => outcome,
        };
        match outcome {
            Outcome::Admitted(operation) => Ok(UnderWay {
                admission: Arc::clone(self),
                operation: Some(operation),
            }),
            Outcome::Refused(refusal) => Err(refusal),
            Outcome::Held | Outcome::Noted | Outcome::Stale => {
                unreachable!("a command no longer held is admitted or refused")
            }
        }
    }

    /// Says what happened, once it is stored.
    pub fn report(&self, report: Report) {
        self.apply(Input::Report(report));
    }

    /// The state a removal holding `workspace` replaced: what it puts back
    /// when refused. `None` when no removal holds it.
    pub fn replaced(&self, workspace: &str) -> Option<Replaced> {
        match self.lock().state.workspace(workspace)?.phase {
            WorkspacePhase::Removing { replaced } => Some(replaced),
            WorkspacePhase::Creating | WorkspacePhase::Ready | WorkspacePhase::Error => None,
        }
    }

    /// Whether a removal or a delete holds `workspace`
    /// ([`State::workspace_held`]).
    pub fn workspace_held(&self, workspace: &str) -> bool {
        self.lock().state.workspace_held(workspace)
    }

    /// Whether a delete of `session` holds `workspace`, or the workspace
    /// went already, with that delete or with a removal.
    pub fn deleting(&self, workspace: &str, session: &str) -> bool {
        match self.lock().state.workspace(workspace) {
            Some(entry) => entry.deleting.as_deref() == Some(session),
            None => true,
        }
    }

    /// Whether admission has session `id`: admitted, and its row not gone.
    pub fn has_session(&self, id: &str) -> bool {
        self.lock().state.session(id).is_some()
    }

    /// A copy of the state, for tests and diagnostics.
    pub fn snapshot(&self) -> State {
        self.lock().state.clone()
    }

    /// One step under the lock. For a command that is held, also the
    /// channel its answer arrives on.
    fn apply(&self, input: Input) -> (Outcome, Option<oneshot::Receiver<Outcome>>) {
        let mut inner = self.lock();
        let Step { outcome, released } = step(&mut inner.state, input);
        let waiter = match outcome {
            Outcome::Held => {
                let (answer, waiter) = oneshot::channel();
                inner.waiting.push(answer);
                Some(waiter)
            }
            Outcome::Admitted(_) | Outcome::Refused(_) | Outcome::Noted | Outcome::Stale => None,
        };
        answer_released(&mut inner, released);
        (outcome, waiter)
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while the lock is held cannot leave the state half
        // changed in a way a later step would trip on: each step is its own
        // whole change, so the state is used as it is.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Gives each released command's answer to whoever waits for it. A command
/// admitted with nobody left to take it is ended at once, as a dropped
/// [`UnderWay`] ends its operation (ARCH-CORE-R16).
fn answer_released(inner: &mut Inner, released: Vec<(usize, Outcome)>) {
    let mut abandoned = Vec::new();
    // From the last place down, so the places before it stay where they are.
    for (place, answer) in released.into_iter().rev() {
        let waiter = inner.waiting.remove(place);
        if let Err(Outcome::Admitted(operation)) = waiter.send(answer) {
            abandoned.push(operation);
        }
    }
    for operation in abandoned {
        let ended = step(&mut inner.state, Input::Report(Report::Ended(operation)));
        answer_released(inner, ended.released);
    }
}

/// An admitted operation. Ending it with [`UnderWay::end`] says how it
/// ended; dropping it any other way says only that it ended
/// ([`Report::Ended`]), so nothing it holds is held for ever
/// (ARCH-CORE-R16).
#[must_use = "dropping it ends the operation"]
pub struct UnderWay {
    admission: Arc<Admission>,
    /// `None` once the operation's end was reported.
    operation: Option<Operation>,
}

impl UnderWay {
    /// Reports how the operation ended, once that is stored.
    pub fn end(mut self, report: Report) {
        self.operation = None;
        self.admission.report(report);
    }
}

impl Drop for UnderWay {
    fn drop(&mut self) {
        if let Some(operation) = self.operation.take() {
            self.admission.report(Report::Ended(operation));
        }
    }
}
