//! Admission's one function, fed inputs directly (ARCH-CORE, Admission): the
//! table that decides every change to which projects, workspaces and sessions
//! exist. No Core runs here and nothing waits: an ordering is a list of
//! inputs.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use ersa_core::admission::{
    Admission, Command, Exclusive, Existing, ExistingWorkspace, Folder, Input, Operation, Outcome,
    ProjectPhase, Refusal, Replaced, Report, State, WorkspacePhase, step,
};

const P: &str = "p1";
const ROOT: &str = "w0";
const W: &str = "w1";

fn folder() -> Folder {
    Folder {
        host: "local".into(),
        key: "c:/work".into(),
    }
}

/// One project with its root workspace and one isolated workspace, both
/// `ready`, and no session.
fn world() -> State {
    State::restored(existing())
}

fn existing() -> Existing {
    Existing {
        projects: vec![(P.into(), folder())],
        workspaces: vec![
            ExistingWorkspace {
                id: ROOT.into(),
                project: P.into(),
                root: true,
                phase: WorkspacePhase::Ready,
            },
            ExistingWorkspace {
                id: W.into(),
                project: P.into(),
                root: false,
                phase: WorkspacePhase::Ready,
            },
        ],
        sessions: Vec::new(),
    }
}

fn command(state: &mut State, command: Command) -> Outcome {
    step(state, Input::Command(command)).outcome
}

fn report(state: &mut State, report: Report) -> Outcome {
    step(state, Input::Report(report)).outcome
}

fn create_session(workspace: &str, session: &str) -> Command {
    Command::CreateSession {
        workspace: workspace.into(),
        session: session.into(),
        found: false,
    }
}

fn find_session(workspace: &str, session: &str) -> Command {
    Command::CreateSession {
        workspace: workspace.into(),
        session: session.into(),
        found: true,
    }
}

fn remove_workspace(workspace: &str) -> Command {
    Command::RemoveWorkspace {
        workspace: workspace.into(),
    }
}

fn remove_project() -> Command {
    Command::RemoveProject { project: P.into() }
}

/// An add of the one folder as `project`, whose root workspace is [`ROOT`]
/// for [`P`].
fn add_project(project: &str) -> Command {
    let root = if project == P {
        ROOT.to_string()
    } else {
        format!("{project}-root")
    };
    Command::AddProject {
        project: project.into(),
        root,
        folder: folder(),
    }
}

fn admitted(operation: Operation) -> Outcome {
    Outcome::Admitted(operation)
}

fn phase(state: &State, workspace: &str) -> Option<WorkspacePhase> {
    state.workspace(workspace).map(|w| w.phase)
}

// -- A session against a workspace's removal, both orders ------------------

// verifies: ARCH-CORE-R14
#[test]
fn a_session_created_first_refuses_the_workspaces_removal() {
    let mut state = world();
    assert_eq!(
        command(&mut state, create_session(W, "s1")),
        admitted(Operation::CreateSession("s1".into()))
    );
    // Counted from the step that admits it, stored or not.
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        Outcome::Refused(Refusal::WorkspaceHasSessions)
    );
    assert_eq!(phase(&state, W), Some(WorkspacePhase::Ready));
}

// verifies: ARCH-CORE-R14
#[test]
fn a_removal_admitted_first_refuses_a_session_in_the_workspace() {
    let mut state = world();
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        admitted(Operation::RemoveWorkspace(W.into()))
    );
    assert_eq!(
        command(&mut state, create_session(W, "s1")),
        Outcome::Refused(Refusal::WorkspaceNotReady)
    );
    // A found session is not recorded there either.
    assert_eq!(
        command(&mut state, find_session(W, "s2")),
        Outcome::Refused(Refusal::WorkspaceNotReady)
    );
    assert!(state.session("s1").is_none() && state.session("s2").is_none());
}

// verifies: FLOW-RESUME-SESSION-R13
#[test]
fn a_found_session_and_its_workspaces_removal_in_both_orders() {
    // Recorded first: it does not hold the removal back, and goes with the
    // workspace.
    let mut state = world();
    command(&mut state, find_session(W, "s1"));
    let stored = Report::SessionStored {
        session: "s1".into(),
    };
    report(&mut state, stored.clone());
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        admitted(Operation::RemoveWorkspace(W.into()))
    );
    report(
        &mut state,
        Report::WorkspaceRemoved {
            workspace: W.into(),
        },
    );
    assert!(state.workspace(W).is_none());
    assert!(state.session("s1").is_none(), "left listed: {state:#?}");
    // What is left of a workspace that went stays held: until Core has let
    // go of the sessions it had loaded there, none of them may start.
    assert!(state.workspace_held(W));

    // Still being stored when the removal is asked for: the removal waits
    // for its row, so the row is there to go with the workspace's.
    let mut state = world();
    command(&mut state, find_session(W, "s1"));
    assert_eq!(command(&mut state, remove_workspace(W)), Outcome::Held);
    let reported = step(&mut state, Input::Report(stored));
    assert_eq!(
        reported.released,
        [(0, admitted(Operation::RemoveWorkspace(W.into())))]
    );

    // A session Ersa starts, admitted meanwhile, answers the waiting
    // removal: it is refused.
    let mut state = world();
    command(&mut state, find_session(W, "s1"));
    command(&mut state, remove_workspace(W));
    let created = step(&mut state, Input::Command(create_session(W, "s2")));
    assert_eq!(
        created.released,
        [(0, Outcome::Refused(Refusal::WorkspaceHasSessions))]
    );

    // The removal first: the session is never recorded.
    let mut state = world();
    command(&mut state, remove_workspace(W));
    assert_eq!(
        command(&mut state, find_session(W, "s1")),
        Outcome::Refused(Refusal::WorkspaceNotReady)
    );
    report(
        &mut state,
        Report::WorkspaceRemoved {
            workspace: W.into(),
        },
    );
    assert!(state.session("s1").is_none());
}

// verifies: FLOW-RESUME-SESSION-R13
#[test]
fn a_found_session_and_its_projects_removal_in_both_orders() {
    // Recorded in the project's own folder first: it does not hold the
    // removal back, and goes with the project.
    let mut state = bare_project();
    command(&mut state, find_session(ROOT, "s1"));
    report(
        &mut state,
        Report::SessionStored {
            session: "s1".into(),
        },
    );
    assert_eq!(
        command(&mut state, remove_project()),
        admitted(Operation::RemoveProject(P.into()))
    );
    report(&mut state, Report::ProjectRemoved { project: P.into() });
    assert!(state.project(P).is_none());
    assert!(state.session("s1").is_none(), "left listed: {state:#?}");

    // Still being stored when the removal is asked for: the removal waits
    // for its row. One that was not recorded after all lets it go on too.
    let mut state = bare_project();
    command(&mut state, find_session(ROOT, "s1"));
    assert_eq!(command(&mut state, remove_project()), Outcome::Held);
    let gone = step(
        &mut state,
        Input::Report(Report::SessionGone {
            session: "s1".into(),
        }),
    );
    assert_eq!(
        gone.released,
        [(0, admitted(Operation::RemoveProject(P.into())))]
    );

    // The removal first: the session is never recorded, and when the
    // removal is then refused the next list records it.
    let mut state = bare_project();
    command(&mut state, remove_project());
    assert_eq!(
        command(&mut state, find_session(ROOT, "s1")),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );
    assert!(state.session("s1").is_none());
    report(
        &mut state,
        Report::ProjectRemovalRefused { project: P.into() },
    );
    assert_eq!(
        command(&mut state, find_session(ROOT, "s1")),
        admitted(Operation::CreateSession("s1".into()))
    );
}

#[test]
fn a_workspace_being_made_or_removed_is_busy_for_a_removal() {
    let mut state = world();
    command(&mut state, remove_workspace(W));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        Outcome::Refused(Refusal::WorkspaceBusy)
    );
    command(
        &mut state,
        Command::CreateWorkspace {
            project: P.into(),
            workspace: "w2".into(),
        },
    );
    assert_eq!(
        command(&mut state, remove_workspace("w2")),
        Outcome::Refused(Refusal::WorkspaceBusy)
    );
    assert_eq!(
        command(&mut state, remove_workspace(ROOT)),
        Outcome::Refused(Refusal::RootWorkspace)
    );
    assert_eq!(
        command(&mut state, remove_workspace("w9")),
        Outcome::Refused(Refusal::NoWorkspace)
    );
}

// -- What a refused or failed removal leaves (PROTO-CORE-API-R94) ----------

// verifies: PROTO-CORE-API-R94
#[test]
fn a_removal_git_refuses_puts_back_the_state_it_replaced() {
    let mut state = world();
    command(&mut state, remove_workspace(W));
    assert_eq!(
        phase(&state, W),
        Some(WorkspacePhase::Removing {
            replaced: Replaced::Ready
        })
    );
    report(
        &mut state,
        Report::WorkspaceRemovalRefused {
            workspace: W.into(),
        },
    );
    assert_eq!(phase(&state, W), Some(WorkspacePhase::Ready));
}

// verifies: PROTO-CORE-API-R94
#[test]
fn an_error_mark_set_before_a_removal_is_admitted_stays_when_git_refuses() {
    let mut state = world();
    // As `session.delete` marks a workspace whose files it takes.
    report(
        &mut state,
        Report::WorkspaceErrored {
            workspace: W.into(),
        },
    );
    command(&mut state, remove_workspace(W));
    report(
        &mut state,
        Report::WorkspaceRemovalRefused {
            workspace: W.into(),
        },
    );
    assert_eq!(phase(&state, W), Some(WorkspacePhase::Error));
    assert_eq!(
        command(&mut state, create_session(W, "s1")),
        Outcome::Refused(Refusal::WorkspaceNotReady)
    );
}

// verifies: PROTO-CORE-API-R94
#[test]
fn an_error_mark_set_while_a_removal_holds_the_workspace_stays_when_it_is_refused() {
    let mut state = world();
    // Only a found session is there, so the removal is admitted; its delete
    // then marks the workspace.
    command(&mut state, find_session(W, "s1"));
    report(
        &mut state,
        Report::SessionStored {
            session: "s1".into(),
        },
    );
    command(&mut state, remove_workspace(W));
    report(
        &mut state,
        Report::WorkspaceErrored {
            workspace: W.into(),
        },
    );
    assert_eq!(
        phase(&state, W),
        Some(WorkspacePhase::Removing {
            replaced: Replaced::Error
        })
    );
    report(
        &mut state,
        Report::WorkspaceRemovalRefused {
            workspace: W.into(),
        },
    );
    assert_eq!(phase(&state, W), Some(WorkspacePhase::Error));
}

#[test]
fn a_removal_that_fails_leaves_the_workspace_in_error_and_removable() {
    let mut state = world();
    command(&mut state, remove_workspace(W));
    report(
        &mut state,
        Report::WorkspaceRemovalFailed {
            workspace: W.into(),
        },
    );
    assert_eq!(phase(&state, W), Some(WorkspacePhase::Error));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        admitted(Operation::RemoveWorkspace(W.into()))
    );
    assert_eq!(
        phase(&state, W),
        Some(WorkspacePhase::Removing {
            replaced: Replaced::Error
        })
    );
}

// -- Making a workspace ----------------------------------------------------

#[test]
fn a_workspace_takes_sessions_only_once_it_is_ready() {
    let mut state = world();
    let made = Command::CreateWorkspace {
        project: P.into(),
        workspace: "w2".into(),
    };
    assert_eq!(
        command(&mut state, made),
        admitted(Operation::CreateWorkspace("w2".into()))
    );
    assert_eq!(
        command(&mut state, create_session("w2", "s1")),
        Outcome::Refused(Refusal::WorkspaceNotReady)
    );
    report(
        &mut state,
        Report::WorkspaceReady {
            workspace: "w2".into(),
        },
    );
    assert_eq!(
        command(&mut state, create_session("w2", "s1")),
        admitted(Operation::CreateSession("s1".into()))
    );
}

#[test]
fn a_workspace_whose_making_failed_is_gone_or_kept_in_error() {
    let mut state = world();
    for (id, kept) in [("w2", false), ("w3", true)] {
        command(
            &mut state,
            Command::CreateWorkspace {
                project: P.into(),
                workspace: id.into(),
            },
        );
        report(
            &mut state,
            Report::WorkspaceCreateFailed {
                workspace: id.into(),
                kept,
            },
        );
    }
    assert_eq!(phase(&state, "w2"), None);
    assert_eq!(phase(&state, "w3"), Some(WorkspacePhase::Error));
}

// -- Projects ---------------------------------------------------------------

// verifies: ARCH-CORE-R17
#[test]
fn a_second_add_of_a_folder_is_held_then_refused_with_the_first_ones_project() {
    let mut state = State::default();
    assert_eq!(
        command(&mut state, add_project("p1")),
        admitted(Operation::AddProject("p1".into()))
    );
    assert_eq!(command(&mut state, add_project("p2")), Outcome::Held);
    assert_eq!(state.held().len(), 1);

    let stored = step(
        &mut state,
        Input::Report(Report::ProjectStored {
            project: "p1".into(),
        }),
    );
    assert_eq!(stored.outcome, Outcome::Noted);
    assert_eq!(
        stored.released,
        [(
            0,
            Outcome::Refused(Refusal::AlreadyProject {
                existing: "p1".into()
            })
        )]
    );
    assert!(state.held().is_empty());
    assert_eq!(phase(&state, ROOT), Some(WorkspacePhase::Ready));
}

// verifies: ARCH-CORE-R17
#[test]
fn a_held_add_is_admitted_when_the_first_ends_without_storing_its_project() {
    let mut state = State::default();
    command(&mut state, add_project("p1"));
    command(&mut state, add_project("p2"));
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::AddProject("p1".into()))),
    );
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::AddProject("p2".into())))]
    );
    assert!(state.project("p1").is_none());
    assert_eq!(
        state.project("p2").map(|p| p.phase),
        Some(ProjectPhase::Adding)
    );
}

// verifies: ARCH-CORE-R17
#[test]
fn a_request_about_a_project_whose_add_has_not_reported_waits_for_it() {
    // The project's rows may be stored, and listed, before its add reports.
    let mut state = State::default();
    command(&mut state, add_project(P));
    let made = Command::CreateWorkspace {
        project: P.into(),
        workspace: W.into(),
    };
    assert_eq!(command(&mut state, made), Outcome::Held);
    assert_eq!(
        command(&mut state, create_session(ROOT, "s1")),
        Outcome::Held
    );
    assert_eq!(command(&mut state, remove_project()), Outcome::Held);

    // Answered in the order they arrived: the workspace and the session
    // are admitted, so the removal is refused for the session.
    let stored = step(
        &mut state,
        Input::Report(Report::ProjectStored { project: P.into() }),
    );
    assert_eq!(
        stored.released,
        [
            (0, admitted(Operation::CreateWorkspace(W.into()))),
            (1, admitted(Operation::CreateSession("s1".into()))),
            (2, Outcome::Refused(Refusal::ProjectHasSessions)),
        ]
    );

    // An add that ends with nothing stored answers them as for no project.
    let mut state = State::default();
    command(&mut state, add_project(P));
    command(&mut state, create_session(ROOT, "s1"));
    command(&mut state, remove_project());
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::AddProject(P.into()))),
    );
    assert_eq!(
        ended.released,
        [
            (0, Outcome::Refused(Refusal::NoWorkspace)),
            (1, Outcome::Refused(Refusal::NoProject)),
        ]
    );
    assert!(state.workspace(ROOT).is_none());
}

#[test]
fn a_project_removal_is_refused_for_sessions_in_its_folder_before_its_workspaces() {
    let mut state = world();
    // An isolated workspace alone.
    assert_eq!(
        command(&mut state, remove_project()),
        Outcome::Refused(Refusal::ProjectHasWorkspaces)
    );
    // With a session in the project's own folder too, that is the answer.
    command(&mut state, create_session(ROOT, "s1"));
    assert_eq!(
        command(&mut state, remove_project()),
        Outcome::Refused(Refusal::ProjectHasSessions)
    );
}

/// The world without its isolated workspace: a project that can be removed.
fn bare_project() -> State {
    let mut existing = existing();
    existing.workspaces.truncate(1);
    State::restored(existing)
}

// verifies: ARCH-CORE-R14
#[test]
fn a_project_removal_admitted_first_decides_a_session_and_holds_a_workspace() {
    let mut state = bare_project();
    assert_eq!(
        command(&mut state, remove_project()),
        admitted(Operation::RemoveProject(P.into()))
    );
    // A session in its folder is refused as busy.
    assert_eq!(
        command(&mut state, create_session(ROOT, "s1")),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );
    // A second removal is busy, and a new workspace waits for the first.
    assert_eq!(
        command(&mut state, remove_project()),
        Outcome::Refused(Refusal::ProjectBusy {
            root: Some(ROOT.into())
        })
    );
    let made = Command::CreateWorkspace {
        project: P.into(),
        workspace: "w2".into(),
    };
    assert_eq!(command(&mut state, made), Outcome::Held);

    let removed = step(
        &mut state,
        Input::Report(Report::ProjectRemoved { project: P.into() }),
    );
    assert_eq!(
        removed.released,
        [(0, Outcome::Refused(Refusal::NoProject))]
    );
    assert!(state.project(P).is_none());
    assert!(state.workspace(ROOT).is_none());
}

// verifies: ARCH-CORE-R14
#[test]
fn a_workspace_or_session_admitted_first_refuses_the_projects_removal() {
    let mut state = bare_project();
    command(
        &mut state,
        Command::CreateWorkspace {
            project: P.into(),
            workspace: "w2".into(),
        },
    );
    // Counted while it is still being made.
    assert_eq!(
        command(&mut state, remove_project()),
        Outcome::Refused(Refusal::ProjectHasWorkspaces)
    );

    let mut state = bare_project();
    command(&mut state, create_session(ROOT, "s1"));
    assert_eq!(
        command(&mut state, remove_project()),
        Outcome::Refused(Refusal::ProjectHasSessions)
    );
}

#[test]
fn a_project_removal_that_is_refused_lets_a_held_workspace_be_made() {
    let mut state = bare_project();
    command(&mut state, remove_project());
    command(
        &mut state,
        Command::CreateWorkspace {
            project: P.into(),
            workspace: "w2".into(),
        },
    );
    let refused = step(
        &mut state,
        Input::Report(Report::ProjectRemovalRefused { project: P.into() }),
    );
    assert_eq!(
        refused.released,
        [(0, admitted(Operation::CreateWorkspace("w2".into())))]
    );
    assert_eq!(phase(&state, "w2"), Some(WorkspacePhase::Creating));
}

// -- A delete's reports -----------------------------------------------------

#[test]
fn a_finished_delete_takes_its_workspace_only_once_no_session_is_in_it() {
    let mut state = world();
    command(&mut state, create_session(W, "s1"));
    let deleted = Report::WorkspaceDeleted {
        workspace: W.into(),
    };
    assert_eq!(report(&mut state, deleted.clone()), Outcome::Stale);
    report(
        &mut state,
        Report::SessionGone {
            session: "s1".into(),
        },
    );
    assert_eq!(report(&mut state, deleted), Outcome::Noted);
    assert!(state.workspace(W).is_none());
}

// -- A delete's hold on its workspace, and a session's removal --------------

fn delete_session(session: &str) -> Command {
    Command::DeleteSession {
        session: session.into(),
    }
}

fn remove_session(session: &str) -> Command {
    Command::RemoveSession {
        session: session.into(),
    }
}

/// [`world`] with `sessions` Ersa started, stored, in the isolated
/// workspace.
fn world_with(sessions: &[&str]) -> State {
    let mut existing = existing();
    for session in sessions {
        existing.sessions.push(((*session).into(), W.into(), false));
    }
    State::restored(existing)
}

// verifies: ARCH-CORE-R14
#[test]
fn a_delete_admitted_first_refuses_what_would_change_its_workspace() {
    let mut state = world_with(&["s1", "s2"]);
    assert_eq!(
        command(&mut state, delete_session("s1")),
        admitted(Operation::DeleteSession("s1".into()))
    );
    assert!(state.workspace_held(W));
    let held = state.clone();

    // `session.create` and a fork's session; a session found in the
    // backend's records is not recorded there either.
    for new in [create_session(W, "s3"), find_session(W, "s3")] {
        assert_eq!(
            command(&mut state, new),
            Outcome::Refused(Refusal::WorkspaceHeld)
        );
    }
    // `session.remove` of another session there, a second `session.delete`
    // of another, and the same delete asked again.
    for other in [
        remove_session("s2"),
        delete_session("s2"),
        delete_session("s1"),
    ] {
        assert_eq!(
            command(&mut state, other),
            Outcome::Refused(Refusal::WorkspaceHeld)
        );
    }
    // Refused, each changed nothing.
    assert_eq!(state, held);

    // Another workspace is not held.
    assert_eq!(
        command(&mut state, create_session(ROOT, "s3")),
        admitted(Operation::CreateSession("s3".into()))
    );
}

// verifies: ARCH-CORE-R14
#[test]
fn what_was_admitted_before_a_delete_is_seen_by_it() {
    // A session created first and still being stored: the delete waits for
    // its row, so what the delete counts in the workspace includes it.
    let mut state = world_with(&["s1"]);
    command(&mut state, create_session(W, "s2"));
    assert_eq!(command(&mut state, delete_session("s1")), Outcome::Held);
    assert!(!state.workspace_held(W));
    let stored = step(
        &mut state,
        Input::Report(Report::SessionStored {
            session: "s2".into(),
        }),
    );
    assert_eq!(
        stored.released,
        [(0, admitted(Operation::DeleteSession("s1".into())))]
    );
    assert!(state.workspace_held(W));

    // A removal of another session first: it goes on, and so does the
    // delete.
    let mut state = world_with(&["s1", "s2"]);
    assert_eq!(
        command(&mut state, remove_session("s2")),
        admitted(Operation::RemoveSession("s2".into()))
    );
    assert_eq!(
        command(&mut state, delete_session("s1")),
        admitted(Operation::DeleteSession("s1".into()))
    );

    // Another delete in the workspace first: the second is busy.
    let mut state = world_with(&["s1", "s2"]);
    command(&mut state, delete_session("s2"));
    assert_eq!(
        command(&mut state, delete_session("s1")),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );
}

#[test]
fn a_delete_of_a_session_being_removed_waits_and_then_finds_no_session() {
    let mut state = world_with(&["s1"]);
    command(&mut state, remove_session("s1"));
    // A second removal of it, and a delete of it, wait for the first.
    assert_eq!(command(&mut state, remove_session("s1")), Outcome::Held);
    assert_eq!(command(&mut state, delete_session("s1")), Outcome::Held);
    let gone = step(
        &mut state,
        Input::Report(Report::SessionGone {
            session: "s1".into(),
        }),
    );
    assert_eq!(
        gone.released,
        [
            (0, Outcome::Refused(Refusal::NoSession)),
            (1, Outcome::Refused(Refusal::NoSession)),
        ]
    );

    // A removal that ends without removing it lets the delete go on.
    let mut state = world_with(&["s1"]);
    command(&mut state, remove_session("s1"));
    command(&mut state, delete_session("s1"));
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::RemoveSession("s1".into()))),
    );
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::DeleteSession("s1".into())))]
    );
}

// verifies: ARCH-CORE-R14
#[test]
fn a_delete_and_a_removal_of_its_workspace_or_project_in_both_orders() {
    // Only a session found in the backend's records is there, so the
    // removal is not refused for sessions: it is busy while the delete
    // holds the workspace.
    let mut found_in_isolated = existing();
    found_in_isolated
        .sessions
        .push(("s1".into(), W.into(), true));
    let mut state = State::restored(found_in_isolated.clone());
    command(&mut state, delete_session("s1"));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );
    // The removal first: the delete is busy.
    let mut state = State::restored(found_in_isolated);
    command(&mut state, remove_workspace(W));
    assert_eq!(
        command(&mut state, delete_session("s1")),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );

    // The same in a project's own folder.
    let mut found_in_root = existing();
    found_in_root.workspaces.truncate(1);
    found_in_root
        .sessions
        .push(("s1".into(), ROOT.into(), true));
    let mut state = State::restored(found_in_root.clone());
    command(&mut state, delete_session("s1"));
    assert_eq!(
        command(&mut state, remove_project()),
        Outcome::Refused(Refusal::ProjectBusy {
            root: Some(ROOT.into())
        })
    );
    let mut state = State::restored(found_in_root);
    command(&mut state, remove_project());
    assert_eq!(
        command(&mut state, delete_session("s1")),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );

    // A session Ersa started there refuses the removal for sessions first,
    // as before the delete was asked for.
    let mut state = world_with(&["s1"]);
    command(&mut state, delete_session("s1"));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        Outcome::Refused(Refusal::WorkspaceHasSessions)
    );
}

// -- A session's one exclusive operation ------------------------------------

fn begin_exclusive(session: &str) -> Command {
    Command::BeginExclusive {
        session: session.into(),
    }
}

fn exclusive(state: &State, session: &str) -> Option<Exclusive> {
    state.session(session).and_then(|s| s.exclusive)
}

// verifies: ARCH-CORE-R14
#[test]
fn a_second_exclusive_operation_waits_and_is_admitted_when_the_first_ends() {
    let mut state = world_with(&["s1", "s2"]);
    assert_eq!(
        command(&mut state, begin_exclusive("s1")),
        admitted(Operation::Exclusive("s1".into()))
    );
    assert_eq!(exclusive(&state, "s1"), Some(Exclusive::Other));
    // Another session's is its own.
    assert_eq!(
        command(&mut state, begin_exclusive("s2")),
        admitted(Operation::Exclusive("s2".into()))
    );
    // A second and a third on the first session wait, in the order they came.
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);

    // The first ends, however its task ended: the second is admitted, and
    // the third still waits, behind it.
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );
    assert_eq!(exclusive(&state, "s1"), Some(Exclusive::Other));
    assert_eq!(state.held().len(), 1);

    // One asked of a session admission does not have is refused.
    assert_eq!(
        command(&mut state, begin_exclusive("s9")),
        Outcome::Refused(Refusal::NoSession)
    );
}

// verifies: ARCH-CORE-R14
#[test]
fn a_removal_of_a_session_and_another_operation_on_it_in_both_orders() {
    // The removal first: the other waits, and finds no session once the
    // removal has removed it.
    let mut state = world_with(&["s1"]);
    command(&mut state, remove_session("s1"));
    assert_eq!(exclusive(&state, "s1"), Some(Exclusive::Removal));
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);
    let gone = step(
        &mut state,
        Input::Report(Report::SessionGone {
            session: "s1".into(),
        }),
    );
    assert_eq!(gone.released, [(0, Outcome::Refused(Refusal::NoSession))]);

    // The removal first, and it ends without removing the session ("it is
    // running; stop it first"): the other goes on.
    let mut state = world_with(&["s1"]);
    command(&mut state, remove_session("s1"));
    command(&mut state, begin_exclusive("s1"));
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::RemoveSession("s1".into()))),
    );
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );

    // The other first: the removal waits for it, and is admitted when it
    // ends.
    let mut state = world_with(&["s1"]);
    command(&mut state, begin_exclusive("s1"));
    assert_eq!(command(&mut state, remove_session("s1")), Outcome::Held);
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::RemoveSession("s1".into())))]
    );

    // A delete of the session does not wait for the other operation, since
    // a delete holds the workspace, not the session. Its admission answers
    // the removal waiting there: the workspace is held now, so it is busy.
    let mut state = world_with(&["s1"]);
    command(&mut state, begin_exclusive("s1"));
    command(&mut state, remove_session("s1"));
    let deleted = step(&mut state, Input::Command(delete_session("s1")));
    assert_eq!(
        deleted.outcome,
        admitted(Operation::DeleteSession("s1".into()))
    );
    assert_eq!(
        deleted.released,
        [(0, Outcome::Refused(Refusal::WorkspaceHeld))]
    );
}

#[test]
fn an_operation_asked_of_a_session_still_being_stored_waits_for_its_row() {
    let mut state = world();
    command(&mut state, create_session(W, "s1"));
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);
    let stored = step(
        &mut state,
        Input::Report(Report::SessionStored {
            session: "s1".into(),
        }),
    );
    assert_eq!(
        stored.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );
}

// -- Every input in every state (ARCH-CORE-R18) -----------------------------

/// Every command and report over a world of one project in one folder, its
/// root and one isolated workspace, and `sessions`.
fn every_input(sessions: &[&str]) -> Vec<Input> {
    let mut inputs = Vec::new();
    let workspaces = [ROOT, W];
    inputs.push(Input::Command(add_project(P)));
    inputs.push(Input::Command(remove_project()));
    for workspace in workspaces {
        inputs.push(Input::Command(remove_workspace(workspace)));
        for &session in sessions {
            for found in [false, true] {
                inputs.push(Input::Command(Command::CreateSession {
                    workspace: workspace.into(),
                    session: session.into(),
                    found,
                }));
            }
        }
    }
    for &session in sessions {
        inputs.push(Input::Command(delete_session(session)));
        inputs.push(Input::Command(remove_session(session)));
        inputs.push(Input::Command(begin_exclusive(session)));
    }
    inputs.push(Input::Command(Command::CreateWorkspace {
        project: P.into(),
        workspace: W.into(),
    }));

    let mut reports = vec![
        Report::ProjectStored { project: P.into() },
        Report::ProjectRemoved { project: P.into() },
        Report::ProjectRemovalRefused { project: P.into() },
        Report::Ended(Operation::AddProject(P.into())),
        Report::Ended(Operation::RemoveProject(P.into())),
    ];
    for workspace in workspaces {
        let workspace = workspace.to_string();
        reports.extend([
            Report::WorkspaceReady {
                workspace: workspace.clone(),
            },
            Report::WorkspaceCreateFailed {
                workspace: workspace.clone(),
                kept: false,
            },
            Report::WorkspaceCreateFailed {
                workspace: workspace.clone(),
                kept: true,
            },
            Report::WorkspaceRemoved {
                workspace: workspace.clone(),
            },
            Report::WorkspaceRemovalRefused {
                workspace: workspace.clone(),
            },
            Report::WorkspaceRemovalFailed {
                workspace: workspace.clone(),
            },
            Report::WorkspaceErrored {
                workspace: workspace.clone(),
            },
            Report::WorkspaceDeleted {
                workspace: workspace.clone(),
            },
            Report::Ended(Operation::CreateWorkspace(workspace.clone())),
            Report::Ended(Operation::RemoveWorkspace(workspace)),
        ]);
    }
    for &session in sessions {
        let session = session.to_string();
        reports.extend([
            Report::SessionStored {
                session: session.clone(),
            },
            Report::SessionGone {
                session: session.clone(),
            },
            Report::Ended(Operation::CreateSession(session.clone())),
            Report::Ended(Operation::DeleteSession(session.clone())),
            Report::Ended(Operation::RemoveSession(session.clone())),
            Report::Ended(Operation::Exclusive(session)),
        ]);
    }
    inputs.extend(reports.into_iter().map(Input::Report));
    inputs
}

/// The sessions Ersa started in `workspace`.
fn started_in(state: &State, workspace: &str) -> usize {
    state
        .sessions()
        .filter(|(_, s)| s.workspace == workspace && !s.found)
        .count()
}

/// What must hold of every state, whatever led to it.
fn check_state(state: &State) {
    for (id, session) in state.sessions() {
        assert!(
            state.workspace(&session.workspace).is_some(),
            "session {id} is in no workspace: {state:#?}"
        );
    }
    for (id, workspace) in state.workspaces() {
        let project = state.project(&workspace.project);
        assert!(
            project.is_some(),
            "workspace {id} has no project: {state:#?}"
        );
        let removal = match workspace.phase {
            WorkspacePhase::Removing { .. } => true,
            WorkspacePhase::Creating | WorkspacePhase::Ready | WorkspacePhase::Error => {
                workspace.root && project.map(|p| p.phase) == Some(ProjectPhase::Removing)
            }
        };
        // No workspace is held for removal with a session Ersa started in it.
        assert!(
            !removal || started_in(state, id) == 0,
            "workspace {id} is held with a session Ersa started: {state:#?}"
        );
        // A workspace has one holder at most: a removal or a delete.
        assert!(
            !(removal && workspace.deleting.is_some()),
            "workspace {id} is held twice: {state:#?}"
        );
    }
    for (id, project) in state.projects() {
        if project.phase == ProjectPhase::Removing {
            let isolated = state
                .workspaces()
                .filter(|(_, w)| &w.project == id && !w.root)
                .count();
            assert_eq!(
                isolated, 0,
                "a project is removed with workspaces: {state:#?}"
            );
        }
    }
    // Nothing waits that could be answered: a held command is still held
    // when decided again.
    for held in state.held() {
        let mut again = state.clone();
        assert_eq!(
            step(&mut again, Input::Command(held.clone())).outcome,
            Outcome::Held,
            "{held:?} waits though it could be answered: {state:#?}"
        );
    }
}

/// A report that ends each thing under way in `state`, as its handle's
/// drop would, or stores what is being stored.
fn ends_of_what_is_under_way(state: &State) -> Vec<Report> {
    let mut reports = Vec::new();
    for (id, project) in state.projects() {
        match project.phase {
            ProjectPhase::Adding => reports.push(Report::Ended(Operation::AddProject(id.clone()))),
            ProjectPhase::Removing => {
                reports.push(Report::Ended(Operation::RemoveProject(id.clone())));
            }
            ProjectPhase::Present => {}
        }
    }
    for (id, workspace) in state.workspaces() {
        match workspace.phase {
            WorkspacePhase::Creating => {
                reports.push(Report::Ended(Operation::CreateWorkspace(id.clone())));
            }
            WorkspacePhase::Removing { .. } => {
                reports.push(Report::Ended(Operation::RemoveWorkspace(id.clone())));
            }
            WorkspacePhase::Ready | WorkspacePhase::Error => {}
        }
        if let Some(session) = &workspace.deleting {
            reports.push(Report::Ended(Operation::DeleteSession(session.clone())));
        }
    }
    for (id, session) in state.sessions() {
        if !session.stored {
            reports.push(Report::Ended(Operation::CreateSession(id.clone())));
        }
        match session.exclusive {
            Some(Exclusive::Removal) => {
                reports.push(Report::Ended(Operation::RemoveSession(id.clone())));
            }
            Some(Exclusive::Other) => {
                reports.push(Report::Ended(Operation::Exclusive(id.clone())));
            }
            None => {}
        }
    }
    reports
}

/// No held command waits for ever, or on another in a cycle: once
/// everything under way has ended, and what the released commands began
/// has ended in turn, nothing is held (ARCH-CORE-R17).
fn check_every_held_command_is_answered(state: &State) {
    if state.held().is_empty() {
        return;
    }
    let mut settling = state.clone();
    for _ in 0..16 {
        if settling.held().is_empty() {
            return;
        }
        let ends = ends_of_what_is_under_way(&settling);
        assert!(
            !ends.is_empty(),
            "held with nothing under way to wait for: {settling:#?}\nfrom {state:#?}"
        );
        for end in ends {
            step(&mut settling, Input::Report(end));
        }
    }
    panic!("still held after everything ended: {settling:#?}\nfrom {state:#?}");
}

/// What must hold of every step from `before` to `after`.
fn check_step(before: &State, input: &Input, outcome: &Outcome, after: &State) {
    let context = || format!("{input:?} -> {outcome:?}\nbefore {before:#?}\nafter {after:#?}");
    // A refusal, a hold's own state aside, and a report that does not fit
    // change nothing.
    match outcome {
        Outcome::Refused(_) | Outcome::Stale => assert_eq!(before, after, "{}", context()),
        Outcome::Admitted(_) | Outcome::Held | Outcome::Noted => {}
    }
    // No input takes a workspace from `error` to `ready`.
    for (id, workspace) in before.workspaces() {
        let was_error = matches!(
            workspace.phase,
            WorkspacePhase::Error
                | WorkspacePhase::Removing {
                    replaced: Replaced::Error
                }
        );
        let now = after.workspace(id).map(|w| w.phase);
        assert!(
            !(was_error && now == Some(WorkspacePhase::Ready)),
            "{id} went from error to ready: {}",
            context()
        );
        // No workspace goes with a session Ersa started in it.
        assert!(
            now.is_some() || started_in(before, id) == 0,
            "{id} went with a session Ersa started: {}",
            context()
        );
    }
    // No session, found ones included, is admitted into a workspace that is
    // not ready or is held: by its removal, by its project's, or by a
    // delete.
    if let (Input::Command(Command::CreateSession { workspace, .. }), Outcome::Admitted(_)) =
        (input, outcome)
    {
        let entry = before.workspace(workspace).expect("an admitted workspace");
        assert_eq!(entry.phase, WorkspacePhase::Ready, "{}", context());
        assert!(!before.workspace_held(workspace), "{}", context());
        assert_eq!(
            before.project(&entry.project).map(|p| p.phase),
            Some(ProjectPhase::Present),
            "{}",
            context()
        );
    }
    // A delete, a removal of a session, and a removal of a workspace are
    // admitted only where nothing holds the workspace.
    let about = match (input, outcome) {
        (
            Input::Command(Command::DeleteSession { session } | Command::RemoveSession { session }),
            Outcome::Admitted(_),
        ) => before.session(session).map(|s| s.workspace.clone()),
        (Input::Command(Command::RemoveWorkspace { workspace }), Outcome::Admitted(_)) => {
            Some(workspace.clone())
        }
        _ => None,
    };
    if let Some(workspace) = about {
        assert!(!before.workspace_held(&workspace), "{}", context());
    }
    // A session has one exclusive operation at most: one is admitted only
    // on a session with none.
    if let (
        Input::Command(Command::RemoveSession { session } | Command::BeginExclusive { session }),
        Outcome::Admitted(_),
    ) = (input, outcome)
    {
        assert_eq!(exclusive(before, session), None, "{}", context());
    }
}

// verifies: ARCH-CORE-R18
// verifies: ARCH-CORE-R15
// verifies: ARCH-CORE-R17
#[test]
fn every_input_in_every_reachable_state_keeps_what_admission_promises() {
    // Two sessions meet every pair of things a workspace's sessions can be
    // at once (started by Ersa or found, stored or not, with an exclusive
    // operation or none, being deleted); a third multiplies the states past
    // a minute's run and adds no pair. One held command at a time meets
    // every way a command is held and answered; how several queue is the
    // fed sequences' above, which hold up to three.
    let sessions = ["s1", "s2"];
    const HELD: usize = 1;
    let inputs = every_input(&sessions);
    let mut seen: HashSet<State> = HashSet::new();
    let mut queue: VecDeque<State> = VecDeque::new();
    for start in [State::default(), world()] {
        if seen.insert(start.clone()) {
            queue.push_back(start);
        }
    }
    let mut steps = 0usize;
    while let Some(before) = queue.pop_front() {
        check_state(&before);
        check_every_held_command_is_answered(&before);
        for input in &inputs {
            let mut after = before.clone();
            // Every input has an answer in every state: `step` never panics.
            let stepped = step(&mut after, input.clone());
            steps += 1;
            check_step(&before, input, &stepped.outcome, &after);
            // Only a step that changed the state answers a held command.
            match stepped.outcome {
                Outcome::Refused(_) | Outcome::Held | Outcome::Stale => {
                    assert!(stepped.released.is_empty(), "{input:?}");
                }
                Outcome::Admitted(_) | Outcome::Noted => {}
            }
            if after.held().len() <= HELD && seen.insert(after.clone()) {
                queue.push_back(after);
            }
        }
    }
    // The world is not trivially small: every phase is reached.
    assert!(seen.len() > 1_000, "only {} states", seen.len());
    assert!(steps > seen.len());
    for wanted in [
        WorkspacePhase::Creating,
        WorkspacePhase::Ready,
        WorkspacePhase::Error,
        WorkspacePhase::Removing {
            replaced: Replaced::Ready,
        },
        WorkspacePhase::Removing {
            replaced: Replaced::Error,
        },
    ] {
        assert!(
            seen.iter().any(|s| phase(s, W) == Some(wanted)),
            "{wanted:?} was never reached"
        );
    }
    // And so is each hold a delete or a removal of a session takes.
    assert!(
        seen.iter()
            .any(|s| s.workspace(W).is_some_and(|w| w.deleting.is_some()))
    );
    assert!(
        seen.iter()
            .any(|s| s.sessions().any(|(_, session)| session.exclusive.is_some()))
    );
}

// -- Operations under way and waiters (the wrapper around the function) -----

async fn admit(admission: &Arc<Admission>, command: Command) -> ersa_core::admission::UnderWay {
    let waited = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        admission.command(command),
    );
    waited.await.expect("answered in time").expect("admitted")
}

/// Waits until a command is held: it is in admission's state, not answered.
async fn until_held(admission: &Admission) {
    let held = async {
        while admission.snapshot().held().is_empty() {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), held)
        .await
        .expect("the command is held in time");
}

// verifies: ARCH-CORE-R16
#[tokio::test]
async fn an_operation_dropped_without_a_report_is_ended() {
    let admission = Arc::new(Admission::default());
    admission.restore(existing());

    // A removal whose task ended without saying how: the workspace is left
    // in error, not held for ever.
    let removal = admit(&admission, remove_workspace(W)).await;
    drop(removal);
    assert_eq!(phase(&admission.snapshot(), W), Some(WorkspacePhase::Error));

    // A session admitted and never stored is not left in its workspace.
    let session = admit(&admission, create_session(ROOT, "s1")).await;
    drop(session);
    assert!(admission.snapshot().session("s1").is_none());

    // One that reported its end is not ended again when dropped.
    let session = admit(&admission, create_session(ROOT, "s2")).await;
    session.end(Report::SessionStored {
        session: "s2".into(),
    });
    assert!(admission.snapshot().session("s2").is_some_and(|s| s.stored));
}

// verifies: ARCH-CORE-R17
#[tokio::test]
async fn a_held_command_is_answered_when_what_it_waits_for_reports() {
    let admission = Arc::new(Admission::default());
    let first = admit(&admission, add_project("p1")).await;
    let second = tokio::spawn({
        let admission = Arc::clone(&admission);
        async move { admission.command(add_project("p2")).await.map(|_| ()) }
    });
    until_held(&admission).await;
    first.end(Report::ProjectStored {
        project: "p1".into(),
    });
    let answered = tokio::time::timeout(std::time::Duration::from_secs(5), second)
        .await
        .expect("answered in time")
        .expect("joined");
    assert_eq!(
        answered,
        Err(Refusal::AlreadyProject {
            existing: "p1".into()
        })
    );
}

// verifies: ARCH-CORE-R16
#[tokio::test]
async fn a_delete_whose_task_ends_without_a_report_releases_its_workspace() {
    let admission = Arc::new(Admission::default());
    let mut stored = existing();
    stored.sessions.push(("s1".into(), W.into(), false));
    admission.restore(stored);

    let deleting = admit(&admission, delete_session("s1")).await;
    assert!(admission.workspace_held(W));
    assert!(admission.deleting(W, "s1"));
    // The delete's task ends, by a refusal, a failure or a panic: its
    // handle is dropped, and nothing more is said.
    drop(deleting);
    assert!(!admission.workspace_held(W));
    assert!(!admission.deleting(W, "s1"));
    // The workspace takes a session again.
    let created = admit(&admission, create_session(W, "s2")).await;
    drop(created);
}

// verifies: ARCH-CORE-R16
#[tokio::test]
async fn a_held_command_admitted_after_its_caller_left_is_ended_at_once() {
    let admission = Arc::new(Admission::default());
    let first = admit(&admission, add_project("p1")).await;
    let second = tokio::spawn({
        let admission = Arc::clone(&admission);
        async move { admission.command(add_project("p2")).await.map(|_| ()) }
    });
    until_held(&admission).await;
    // The caller stops waiting; then the first add ends without a project.
    second.abort();
    let _ = second.await;
    drop(first);
    // The second add was admitted with nobody to take it, so it is
    // ended too: the folder is free again.
    let state = admission.snapshot();
    assert!(state.project("p1").is_none() && state.project("p2").is_none());
    assert!(state.held().is_empty());
}
