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
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionEntry {
    pub workspace: Id,
    /// Found in the backend's records rather than started by Ersa: it never
    /// holds its workspace back, and goes with it.
    pub found: bool,
    /// Whether its row is stored yet.
    pub stored: bool,
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
    /// `root_runs_found`: whether Core is running a session found in the
    /// backend's records in the project's own folder, which holds the
    /// removal back as a session Ersa started does.
    RemoveProject {
        project: Id,
        root_runs_found: bool,
    },
    CreateWorkspace {
        project: Id,
        workspace: Id,
    },
    /// `runs_found`: as [`Command::RemoveProject`]'s flag, for this workspace.
    RemoveWorkspace {
        workspace: Id,
        runs_found: bool,
    },
    /// A new session in `workspace`: one Ersa starts, or one `found` in the
    /// backend's records.
    CreateSession {
        workspace: Id,
        session: Id,
        found: bool,
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
    /// A session Ersa started, or one Core is running, is in its folder.
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
    /// The workspace is the root of a project being removed.
    WorkspaceHeld,
    /// The id is taken.
    SessionExists,
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
            if outcome == Outcome::Held {
                state.held.push(command);
            }
            Step {
                outcome,
                released: Vec::new(),
            }
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

/// Decides every held command again, in the order they arrived. One pass is
/// enough: admitting a command never frees what another waits for, only a
/// report does.
fn release(state: &mut State) -> Vec<(usize, Outcome)> {
    let waiting = std::mem::take(&mut state.held);
    let mut released = Vec::new();
    for (place, command) in waiting.into_iter().enumerate() {
        match decide(state, &command) {
            Outcome::Held => state.held.push(command),
            answer @ (Outcome::Admitted(_)
            | Outcome::Refused(_)
            | Outcome::Noted
            | Outcome::Stale) => released.push((place, answer)),
        }
    }
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
                    };
                    state.workspaces.insert(root.clone(), entry);
                    Outcome::Admitted(Operation::AddProject(project.clone()))
                }
            }
        }
        Command::RemoveProject {
            project,
            root_runs_found,
        } => {
            let root = state.root_of(project).cloned();
            match state.projects.get(project).map(|entry| entry.phase) {
                None => Outcome::Refused(Refusal::NoProject),
                // Stored, perhaps, and its add yet to report: whether there
                // is a project to remove is known once it does.
                Some(ProjectPhase::Adding) => Outcome::Held,
                Some(ProjectPhase::Removing) => Outcome::Refused(Refusal::ProjectBusy { root }),
                Some(ProjectPhase::Present) => {
                    let sessions = *root_runs_found
                        || root
                            .as_ref()
                            .is_some_and(|root| state.has_started_session(root));
                    let isolated = state
                        .workspaces
                        .values()
                        .any(|workspace| &workspace.project == project && !workspace.root);
                    if sessions {
                        Outcome::Refused(Refusal::ProjectHasSessions)
                    } else if isolated {
                        Outcome::Refused(Refusal::ProjectHasWorkspaces)
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
                    };
                    state.workspaces.insert(workspace.clone(), entry);
                    Outcome::Admitted(Operation::CreateWorkspace(workspace.clone()))
                }
            }
        }
        Command::RemoveWorkspace {
            workspace,
            runs_found,
        } => {
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
            if *runs_found || state.has_started_session(workspace) {
                return Outcome::Refused(Refusal::WorkspaceHasSessions);
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
            match state.projects.get(&entry.project).map(|entry| entry.phase) {
                Some(ProjectPhase::Present) => {}
                // The folder is on its way out. A session found there is
                // recorded all the same, and goes with the project.
                Some(ProjectPhase::Removing) => {
                    if !found {
                        return Outcome::Refused(Refusal::WorkspaceHeld);
                    }
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
            };
            state.sessions.insert(session.clone(), entry);
            Outcome::Admitted(Operation::CreateSession(session.clone()))
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
