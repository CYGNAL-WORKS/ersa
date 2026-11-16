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
use std::time::Duration;

use ersa_protocol::core_api::Deleting;
use tokio::sync::oneshot;

use crate::persistence::SessionRow;

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
    /// Its row as last stored: the one copy Core keeps in memory. `None`
    /// until the row is stored.
    pub row: Option<SessionRow>,
    /// Whether a `session.remove` of it is under way: asked for, and
    /// perhaps still waiting for a run that is winding down, before it
    /// takes the session's exclusive operation.
    pub removing: bool,
    /// Whether its one exclusive operation is under way: a stop, a start,
    /// a view being opened, an archive, an arrangement, a delete's mark, or
    /// a removal once it goes on. A session has at most one, and another
    /// asked for meanwhile waits.
    pub exclusive: bool,
    /// Whether Core runs its backend.
    pub run: Run,
}

impl SessionEntry {
    /// Whether its row is stored yet.
    pub fn stored(&self) -> bool {
        self.row.is_some()
    }
}

/// Whether Core runs a session's backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Run {
    Stopped,
    /// Launched, starting or ready.
    Running,
    /// Its end was announced, and Core has not let go of the run yet: a
    /// start or a removal asked for now waits for that.
    Ending,
}

/// Everything admission decides from. Ordered maps and no counters, so two
/// states reached by different routes compare equal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct State {
    projects: BTreeMap<Id, ProjectEntry>,
    workspaces: BTreeMap<Id, WorkspaceEntry>,
    sessions: BTreeMap<Id, SessionEntry>,
    /// Whether Core is shutting down: from then nothing starts.
    closing: bool,
    /// Commands waiting for a report, in the order they arrived.
    held: Vec<Command>,
}

/// What the store holds when Core starts.
#[derive(Clone, Debug, Default)]
pub struct Existing {
    pub projects: Vec<(Id, Folder)>,
    pub workspaces: Vec<ExistingWorkspace>,
    /// Each session's row.
    pub sessions: Vec<SessionRow>,
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
        for row in existing.sessions {
            state.sessions.insert(
                row.id.clone(),
                SessionEntry {
                    workspace: row.workspace_id.clone(),
                    found: row.started_elsewhere,
                    row: Some(row),
                    removing: false,
                    exclusive: false,
                    run: Run::Stopped,
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

    /// Session `id`'s row as last stored; `None` for a session that is
    /// gone, or whose row is not stored yet.
    pub fn row(&self, id: &str) -> Option<&SessionRow> {
        self.sessions.get(id)?.row.as_ref()
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

    /// Whether Core is shutting down.
    pub fn closing(&self) -> bool {
        self.closing
    }

    /// Whether a session found in the backend's records in `workspace` has
    /// an exclusive operation under way, which may be a start: a removal
    /// waits to see what it comes to.
    fn has_busy_found_session(&self, workspace: &str) -> bool {
        self.sessions
            .values()
            .any(|session| session.workspace == workspace && session.found && session.exclusive)
    }

    /// Whether Core runs a session found in the backend's records in
    /// `workspace`, its run winding down included, which holds back
    /// removing the workspace as a session Ersa started does.
    fn runs_found_session(&self, workspace: &str) -> bool {
        self.sessions.values().any(|session| {
            session.workspace == workspace
                && session.found
                && match session.run {
                    Run::Running | Run::Ending => true,
                    Run::Stopped => false,
                }
        })
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
            .any(|session| session.workspace == workspace && !session.stored())
    }

    /// Whether a removal holds `workspace`: its own, or its project's when
    /// it is the project's own folder.
    fn held_for_removal(&self, workspace: &str) -> bool {
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
        removing || project_going
    }

    /// Whether something holds `workspace`: its own removal, its project's
    /// removal when it is the project's own folder, or a session's delete.
    /// A held workspace takes no session, and no other removal or delete.
    /// One admission does not have counts as held: it went with a removal
    /// or a delete, and nothing may start in what is left of it.
    pub fn workspace_held(&self, workspace: &str) -> bool {
        self.held_for_removal(workspace)
            || self
                .workspaces
                .get(workspace)
                .is_some_and(|entry| entry.deleting.is_some())
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
    /// `session.remove`. It takes the session's exclusive operation as a
    /// second step, once any run that is winding down has finished.
    RemoveSession {
        session: Id,
    },
    /// The session's exclusive operation: admitted when none is under way
    /// on it, held until then.
    BeginExclusive {
        session: Id,
    },
    /// The exclusive operation of a start (a resume, or a send to a stopped
    /// session): as [`Command::BeginExclusive`], and held while a run of
    /// the session is winding down, refused while one runs or Core is
    /// closing.
    BeginStart {
        session: Id,
    },
    /// The exclusive operation of a `session.remove` under way: as
    /// [`Command::BeginExclusive`], and held while a run of the session is
    /// winding down, refused while one runs.
    BeginRemoval {
        session: Id,
    },
    /// The backend is about to be started: the last moment a start can be
    /// refused for shutdown. Admitted, the run is the operation.
    Launch {
        session: Id,
    },
    /// Asks whether Core runs the session, once a run that is winding down
    /// has been let go of: held until then, done when it is stopped,
    /// refused, as running, when it runs. It begins nothing.
    AwaitRunEnd {
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
    /// A session's run, from its launch until Core has let go of it.
    Run(Id),
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
    /// The session's row is stored: this is it.
    SessionStored {
        row: Box<SessionRow>,
    },
    /// A change to the session's row is stored.
    RowWritten {
        session: Id,
        change: RowChange,
    },
    /// The section is gone from the store, and with it from every session's
    /// row there.
    SectionRemoved {
        section: Id,
    },
    /// The session's row is gone.
    SessionGone {
        session: Id,
    },
    /// The end of the session's run was announced: it is winding down.
    RunEnding {
        session: Id,
    },
    /// Core is shutting down.
    Closing,
    /// Whoever waited for the held command at `place` in the queue has
    /// stopped waiting: its time limit passed.
    GaveUp {
        place: usize,
    },
    /// The operation's task ended without saying how (ARCH-CORE-R16).
    Ended(Operation),
}

/// One stored change to a session's row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RowChange {
    Archived(bool),
    /// Pinned or not, and its section (`Some(None)`: no section), each
    /// changed only when given.
    Arranged {
        pinned: Option<bool>,
        section: Option<Option<Id>>,
    },
    /// Where the session's delete is.
    Deleting(Deleting),
    Mode(String),
    /// The backend's own id for the session.
    BackendId(String),
    /// When the session's latest turn ended.
    LastActivity(String),
    /// A value Core gave the backend, and when.
    Requested {
        setting: Setting,
        value: String,
        at: String,
    },
    /// The fork has a record of its own: what it was copied from is
    /// forgotten.
    ForkLinkCleared,
}

/// Which of a session's values Core gave its backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Setting {
    Model,
    Effort,
    PermissionMode,
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
    /// A session Ersa started is in its folder, or one found in the
    /// backend's records that Core runs.
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
    /// Core is shutting down; nothing starts.
    Closing,
    /// Core runs the session's backend.
    Running,
    /// What the command waited for did not come within its time limit.
    GaveUp,
}

/// Admission's answer to one input.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// A command: the operation may begin, and is recorded.
    Admitted(Operation),
    /// A command that only asks: answered, with nothing begun or changed.
    Done,
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
                Outcome::Refused(_) | Outcome::Done | Outcome::Noted | Outcome::Stale => Vec::new(),
            };
            Step { outcome, released }
        }
        // A held command's time limit: it is answered and leaves the queue,
        // and nothing else changes.
        Input::Report(Report::GaveUp { place }) => {
            if place < state.held.len() {
                state.held.remove(place);
                Step {
                    outcome: Outcome::Noted,
                    released: vec![(place, Outcome::Refused(Refusal::GaveUp))],
                }
            } else {
                Step {
                    outcome: Outcome::Stale,
                    released: Vec::new(),
                }
            }
        }
        Input::Report(report) => {
            let outcome = note(state, report);
            // A report that changed something may be what a held command
            // waits for.
            let released = match outcome {
                Outcome::Noted => release(state),
                Outcome::Stale
                | Outcome::Admitted(_)
                | Outcome::Done
                | Outcome::Refused(_)
                | Outcome::Held => Vec::new(),
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
                | Outcome::Done
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
                    let busy_found = root
                        .as_ref()
                        .is_some_and(|root| state.has_busy_found_session(root));
                    let runs_found = root
                        .as_ref()
                        .is_some_and(|root| state.runs_found_session(root));
                    if sessions {
                        Outcome::Refused(Refusal::ProjectHasSessions)
                    } else if isolated {
                        Outcome::Refused(Refusal::ProjectHasWorkspaces)
                    } else if deleting {
                        // A delete of a session found there holds the folder.
                        Outcome::Refused(Refusal::ProjectBusy { root })
                    } else if arriving || busy_found {
                        Outcome::Held
                    } else if runs_found {
                        // One found there that Core runs holds it back too.
                        Outcome::Refused(Refusal::ProjectHasSessions)
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
            // What a session found there is about to be is not known yet:
            // one still being stored, or one with an operation under way,
            // which may be a start.
            if state.has_arriving_session(workspace) || state.has_busy_found_session(workspace) {
                return Outcome::Held;
            }
            // One found there that Core runs holds it back, as one Ersa
            // started does: no backend is left running in a removed folder.
            if state.runs_found_session(workspace) {
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
            if state.closing {
                return Outcome::Refused(Refusal::Closing);
            }
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
                row: None,
                removing: false,
                exclusive: false,
                run: Run::Stopped,
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
            if entry.removing || state.has_arriving_session(&workspace) {
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
            // Its row is still being stored, or another removal of it is
            // under way: whether there is a session to remove is known once
            // that reports.
            if !entry.stored() || entry.removing {
                return Outcome::Held;
            }
            if let Some(entry) = state.sessions.get_mut(session) {
                entry.removing = true;
            }
            Outcome::Admitted(Operation::RemoveSession(session.clone()))
        }
        Command::BeginExclusive { session } => {
            let Some(entry) = state.sessions.get_mut(session) else {
                return Outcome::Refused(Refusal::NoSession);
            };
            // One at a time: the next waits for the one under way to end.
            // So does one asked of a session whose row is being stored.
            if !entry.stored() || entry.exclusive {
                return Outcome::Held;
            }
            entry.exclusive = true;
            Outcome::Admitted(Operation::Exclusive(session.clone()))
        }
        Command::BeginStart { session } => begin_after_run(state, session, true),
        Command::BeginRemoval { session } => begin_after_run(state, session, false),
        Command::AwaitRunEnd { session } => {
            match state.sessions.get(session).map(|entry| entry.run) {
                None => Outcome::Refused(Refusal::NoSession),
                Some(Run::Ending) => Outcome::Held,
                Some(Run::Running) => Outcome::Refused(Refusal::Running),
                Some(Run::Stopped) => Outcome::Done,
            }
        }
        Command::Launch { session } => {
            let closing = state.closing;
            let going = state
                .sessions
                .get(session)
                .is_some_and(|entry| state.held_for_removal(&entry.workspace));
            let Some(entry) = state.sessions.get_mut(session) else {
                return Outcome::Refused(Refusal::NoSession);
            };
            if closing {
                return Outcome::Refused(Refusal::Closing);
            }
            // No backend starts in a folder on its way out. (A delete's
            // hold does not stop one: the delete counted the session.)
            if going {
                return Outcome::Refused(Refusal::WorkspaceHeld);
            }
            match entry.run {
                Run::Running | Run::Ending => return Outcome::Refused(Refusal::Running),
                Run::Stopped => {}
            }
            entry.run = Run::Running;
            Outcome::Admitted(Operation::Run(session.clone()))
        }
    }
}

/// The exclusive operation of a start (`starts`) or of a removal under way.
/// First its turn: what the operation under way leaves is what it is
/// decided against, so a stop under way on a running session leaves it
/// stopped, and the start then starts it, the removal removes it. Then the
/// run: held while it is ending (a client may have seen the session end
/// before Core has let go of the run), refused while it runs. A start is
/// refused once Core is closing.
fn begin_after_run(state: &mut State, session: &Id, starts: bool) -> Outcome {
    let closing = state.closing;
    let Some(entry) = state.sessions.get_mut(session) else {
        return Outcome::Refused(Refusal::NoSession);
    };
    if !entry.stored() || entry.exclusive {
        return Outcome::Held;
    }
    match entry.run {
        Run::Ending => return Outcome::Held,
        Run::Running => return Outcome::Refused(Refusal::Running),
        Run::Stopped => {}
    }
    if starts && closing {
        return Outcome::Refused(Refusal::Closing);
    }
    entry.exclusive = true;
    Outcome::Admitted(Operation::Exclusive(session.clone()))
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
        // The row enters the state in the step that says it is stored: the
        // row of the session that was admitted, in its workspace. A second
        // report for one session never replaces its row.
        Report::SessionStored { row } => match state.sessions.get_mut(&row.id) {
            Some(entry)
                if !entry.stored()
                    && entry.workspace == row.workspace_id
                    && entry.found == row.started_elsewhere =>
            {
                entry.row = Some(*row);
                Outcome::Noted
            }
            Some(_) | None => Outcome::Stale,
        },
        // A write for a session that is gone keeps nothing.
        Report::RowWritten { session, change } => {
            match state
                .sessions
                .get_mut(&session)
                .and_then(|entry| entry.row.as_mut())
            {
                Some(row) => {
                    write(row, change);
                    Outcome::Noted
                }
                None => Outcome::Stale,
            }
        }
        // One step, so no row is left naming a section that is gone.
        Report::SectionRemoved { section } => {
            for row in state
                .sessions
                .values_mut()
                .filter_map(|entry| entry.row.as_mut())
            {
                if row.section_id.as_deref() == Some(section.as_str()) {
                    row.section_id = None;
                }
            }
            Outcome::Noted
        }
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
                match state.sessions.get(&session).map(SessionEntry::stored) {
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
            Operation::RemoveSession(session) => match state.sessions.get_mut(&session) {
                Some(entry) if entry.removing => {
                    entry.removing = false;
                    Outcome::Noted
                }
                Some(_) | None => Outcome::Stale,
            },
            Operation::Exclusive(session) => match state.sessions.get_mut(&session) {
                Some(entry) if entry.exclusive => {
                    entry.exclusive = false;
                    Outcome::Noted
                }
                Some(_) | None => Outcome::Stale,
            },
            // Core has let go of the run, however its task ended.
            Operation::Run(session) => match state.sessions.get_mut(&session) {
                Some(entry) => match entry.run {
                    Run::Running | Run::Ending => {
                        entry.run = Run::Stopped;
                        Outcome::Noted
                    }
                    Run::Stopped => Outcome::Stale,
                },
                None => Outcome::Stale,
            },
        },
        Report::RunEnding { session } => match state.sessions.get_mut(&session) {
            Some(entry) => match entry.run {
                Run::Running => {
                    entry.run = Run::Ending;
                    Outcome::Noted
                }
                Run::Stopped | Run::Ending => Outcome::Stale,
            },
            None => Outcome::Stale,
        },
        Report::Closing => {
            if state.closing {
                Outcome::Stale
            } else {
                state.closing = true;
                Outcome::Noted
            }
        }
        // Answered in `step`, which has the queue.
        Report::GaveUp { .. } => Outcome::Stale,
    }
}

/// Moves `workspace` to the phase `next` gives for the one it is in;
/// `Stale` when there is no such workspace or `next` gives none.
/// Applies one stored change to `row`.
fn write(row: &mut SessionRow, change: RowChange) {
    match change {
        RowChange::Archived(archived) => row.archived = archived,
        RowChange::Arranged { pinned, section } => {
            if let Some(pinned) = pinned {
                row.pinned = pinned;
            }
            if let Some(section) = section {
                row.section_id = section;
            }
        }
        RowChange::Deleting(deleting) => row.deleting = Some(deleting),
        RowChange::Mode(mode) => row.mode = Some(mode),
        RowChange::BackendId(id) => row.backend_session_id = Some(id),
        RowChange::LastActivity(at) => row.last_activity_at = Some(at),
        RowChange::Requested { setting, value, at } => {
            let (stored, stored_at) = match setting {
                Setting::Model => (&mut row.model, &mut row.model_at),
                Setting::Effort => (&mut row.effort, &mut row.effort_at),
                Setting::PermissionMode => (&mut row.permission_mode, &mut row.permission_mode_at),
            };
            *stored = Some(value);
            *stored_at = Some(at);
        }
        RowChange::ForkLinkCleared => {
            row.forked_from = None;
            row.forked_through = None;
        }
    }
}

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
    /// Who waits for each held command, in the order of [`State::held`],
    /// each with a number of its own, so a waiter that gives up withdraws
    /// its own command and no other like it.
    waiting: Vec<(u64, oneshot::Sender<Outcome>)>,
    /// The next waiter's number.
    next_waiter: u64,
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
            Some((_, waiter)) => waiter.await.expect("a held command is always answered"),
            None => outcome,
        };
        self.answered(outcome)
    }

    /// Asks for the exclusive operation of a start of `session`
    /// ([`Command::BeginStart`]), with `limit` on its wait for a run that is
    /// winding down.
    pub async fn begin_start(
        self: &Arc<Self>,
        session: &str,
        limit: Duration,
    ) -> Result<UnderWay, Refusal> {
        let command = Command::BeginStart {
            session: session.to_string(),
        };
        let outcome = self.within(session, command, limit).await;
        self.answered(outcome)
    }

    /// Asks for the exclusive operation of a removal of `session` that is
    /// under way ([`Command::BeginRemoval`]), with `limit` on its wait for a
    /// run that is winding down.
    pub async fn begin_removal(
        self: &Arc<Self>,
        session: &str,
        limit: Duration,
    ) -> Result<UnderWay, Refusal> {
        let command = Command::BeginRemoval {
            session: session.to_string(),
        };
        let outcome = self.within(session, command, limit).await;
        self.answered(outcome)
    }

    /// Asks whether Core runs `session` ([`Command::AwaitRunEnd`]), waiting
    /// up to `limit` for a run that is winding down: `Ok` when it is
    /// stopped, [`Refusal::Running`] when it runs.
    pub async fn run_settled(&self, session: &str, limit: Duration) -> Result<(), Refusal> {
        let command = Command::AwaitRunEnd {
            session: session.to_string(),
        };
        match self.within(session, command, limit).await {
            Outcome::Done => Ok(()),
            Outcome::Refused(refusal) => Err(refusal),
            Outcome::Admitted(_) | Outcome::Held | Outcome::Noted | Outcome::Stale => {
                unreachable!("a question no longer held is answered or refused")
            }
        }
    }

    /// Applies `command`, which may be held behind a run of `session` that
    /// is winding down, and waits for its answer with `limit` on that wait:
    /// when the run has been ending for a whole `limit`, the command gives
    /// up ([`Report::GaveUp`]) and is refused so. A wait for anything else,
    /// another operation under way on the session, has no limit.
    async fn within(&self, session: &str, command: Command, limit: Duration) -> Outcome {
        let (outcome, waiter) = self.apply(Input::Command(command));
        let Some((number, mut waiter)) = waiter else {
            return outcome;
        };
        let ending =
            || self.lock().state.session(session).map(|entry| entry.run) == Some(Run::Ending);
        // Ending at two looks a `limit` apart: at least that long.
        let mut was_ending = ending();
        loop {
            match tokio::time::timeout(limit, &mut waiter).await {
                Ok(answer) => return answer.expect("a held command is always answered"),
                Err(_) => {
                    let still = ending();
                    if still && was_ending {
                        self.give_up(number);
                        // Answered now: "gave up", or what a step just
                        // before decided.
                        return waiter.await.expect("a held command is always answered");
                    }
                    was_ending = still;
                }
            }
        }
    }

    /// The waiter numbered `number` stops waiting: its held command is
    /// answered "gave up" and leaves the queue, and no other command does.
    /// Nothing happens when it was answered already.
    fn give_up(&self, number: u64) {
        let mut inner = self.lock();
        let place = inner
            .waiting
            .iter()
            .position(|(waiter, _)| *waiter == number);
        if let Some(place) = place {
            let gave_up = step(&mut inner.state, Input::Report(Report::GaveUp { place }));
            answer_released(&mut inner, gave_up.released);
        }
    }

    /// Asks to launch `session`'s backend, without waiting: a launch is
    /// admitted or refused in the step, so its caller may hold a lock.
    pub fn launch(self: &Arc<Self>, session: &str) -> Result<UnderWay, Refusal> {
        let command = Command::Launch {
            session: session.to_string(),
        };
        let (outcome, _) = self.apply(Input::Command(command));
        self.answered(outcome)
    }

    /// The handle of an admitted operation, or the refusal.
    fn answered(self: &Arc<Self>, outcome: Outcome) -> Result<UnderWay, Refusal> {
        match outcome {
            Outcome::Admitted(operation) => Ok(UnderWay {
                admission: Arc::clone(self),
                operation: Some(operation),
            }),
            Outcome::Refused(refusal) => Err(refusal),
            Outcome::Done | Outcome::Held | Outcome::Noted | Outcome::Stale => {
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

    /// Session `id`'s row as last stored; `None` once the session is gone.
    pub fn row(&self, id: &str) -> Option<SessionRow> {
        self.lock().state.row(id).cloned()
    }

    /// The rows of the sessions found in workspace `workspace`'s folder
    /// rather than started by Ersa.
    pub fn found_in(&self, workspace: &str) -> Vec<SessionRow> {
        let inner = self.lock();
        let rows = inner
            .state
            .sessions()
            .filter_map(|(_, entry)| entry.row.as_ref());
        rows.filter(|row| row.started_elsewhere && row.workspace_id == workspace)
            .cloned()
            .collect()
    }

    /// Says a change to session `session`'s row is stored. False when the
    /// session is gone: nothing is kept.
    pub fn wrote(&self, session: &str, change: RowChange) -> bool {
        let report = Report::RowWritten {
            session: session.to_string(),
            change,
        };
        match self.apply(Input::Report(report)).0 {
            Outcome::Noted => true,
            Outcome::Stale => false,
            // A report is noted or stale, never the answer to a command.
            Outcome::Admitted(_) | Outcome::Done | Outcome::Refused(_) | Outcome::Held => false,
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

    /// One step under the lock. For a command that is held, also its
    /// waiter's number and the channel its answer arrives on.
    fn apply(&self, input: Input) -> (Outcome, Option<(u64, oneshot::Receiver<Outcome>)>) {
        let mut inner = self.lock();
        let Step { outcome, released } = step(&mut inner.state, input);
        let waiter = match outcome {
            Outcome::Held => {
                let (answer, waiter) = oneshot::channel();
                let number = inner.next_waiter;
                inner.next_waiter += 1;
                inner.waiting.push((number, answer));
                Some((number, waiter))
            }
            Outcome::Admitted(_)
            | Outcome::Done
            | Outcome::Refused(_)
            | Outcome::Noted
            | Outcome::Stale => None,
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
        let (_, waiter) = inner.waiting.remove(place);
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
