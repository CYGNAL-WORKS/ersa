//! Admission's one function, fed inputs directly (ARCH-CORE, Admission): the
//! table that decides every change to which projects, workspaces and sessions
//! exist. No Core runs here and nothing waits: an ordering is a list of
//! inputs.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use ersa_core::admission::{
    Admission, Command, Exclusive, Existing, ExistingWorkspace, Folder, Input, Operation, Outcome,
    ProjectPhase, Refusal, Replaced, Report, RowChange, Run, SectionPhase, SendIs, SendStage,
    Sending, Setting, State, WorkspacePhase, step,
};
use ersa_core::persistence::SessionRow;
use ersa_protocol::core_api::Deleting;

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
        sections: Vec::new(),
    }
}

/// Session `id`'s row as it is first stored, in workspace `workspace`;
/// `found` when it was found in the backend's records.
fn row(id: &str, workspace: &str, found: bool) -> SessionRow {
    SessionRow {
        id: id.into(),
        workspace_id: workspace.into(),
        host_id: "local".into(),
        backend: "fake".into(),
        backend_session_id: None,
        mode: None,
        model: None,
        effort: None,
        permission_mode: None,
        model_at: None,
        effort_at: None,
        permission_mode_at: None,
        cwd: format!("c:/work/{workspace}"),
        archived: false,
        deleting: None,
        pinned: false,
        section_id: None,
        last_activity_at: None,
        forked_from: None,
        forked_through: None,
        started_elsewhere: found,
    }
}

/// The report that a session Ersa started in the isolated workspace has
/// its row stored.
fn session_stored(session: &str) -> Report {
    stored_in(W, session, false)
}

/// The report that session `session` in workspace `workspace` has its row
/// stored; `found` as [`row`] takes it.
fn stored_in(workspace: &str, session: &str, found: bool) -> Report {
    Report::SessionStored {
        row: Box::new(row(session, workspace, found)),
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
    let stored = stored_in(W, "s1", true);
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
    report(&mut state, stored_in(ROOT, "s1", true));
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
    report(&mut state, stored_in(W, "s1", true));
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

// -- A session's row (ARCH-CORE-R14) ----------------------------------------

const SECTION: &str = "sec1";

/// One of each kind of write to a session's row.
fn writes(_session: &str) -> Vec<RowChange> {
    vec![
        RowChange::Archived(true),
        RowChange::Arranged {
            pinned: Some(true),
            section: Some(Some(SECTION.into())),
        },
        RowChange::Deleting(Deleting::Running),
        RowChange::Mode("plan".into()),
        RowChange::BackendId("b-1".into()),
        RowChange::LastActivity("2026-11-16T10:00:00Z".into()),
        RowChange::Requested {
            setting: Setting::Effort,
            value: "high".into(),
            at: "2026-11-16T09:00:00Z".into(),
        },
        RowChange::ForkLinkCleared,
    ]
}

fn wrote(state: &mut State, session: &str, change: RowChange) -> Outcome {
    let session = session.into();
    report(state, Report::RowWritten { session, change })
}

// verifies: ARCH-CORE-R14
#[test]
fn a_session_s_row_enters_the_state_when_it_is_stored_and_leaves_with_the_session() {
    let mut state = world();
    command(&mut state, create_session(W, "s1"));
    assert_eq!(state.row("s1"), None);
    // Only the row of the session that was admitted, in its workspace.
    assert_eq!(
        report(&mut state, stored_in(ROOT, "s1", false)),
        Outcome::Stale
    );
    assert_eq!(report(&mut state, stored_in(W, "s1", true)), Outcome::Stale);
    assert_eq!(state.row("s1"), None);
    assert_eq!(report(&mut state, session_stored("s1")), Outcome::Noted);
    assert_eq!(state.row("s1"), Some(&row("s1", W, false)));

    // A second report never replaces it.
    wrote(&mut state, "s1", RowChange::Archived(true));
    assert_eq!(report(&mut state, session_stored("s1")), Outcome::Stale);
    assert!(state.row("s1").is_some_and(|row| row.archived));

    let gone = Report::SessionGone {
        session: "s1".into(),
    };
    assert_eq!(report(&mut state, gone), Outcome::Noted);
    assert_eq!(state.row("s1"), None);

    // What the store held when Core started is there from the start, and
    // goes with its workspace.
    let mut stored = existing();
    let mut found = row("s2", W, true);
    found.archived = true;
    stored.sessions.push(found.clone());
    let mut state = State::restored(stored);
    assert_eq!(state.row("s2"), Some(&found));
    command(&mut state, remove_workspace(W));
    report(
        &mut state,
        Report::WorkspaceRemoved {
            workspace: W.into(),
        },
    );
    assert_eq!(state.row("s2"), None);
}

// verifies: ARCH-CORE-R14
#[test]
fn each_write_changes_the_row_it_names_and_no_other() {
    let mut state = world_with(&["s1", "s2"]);
    let untouched = row("s2", W, false);
    let mut expected = row("s1", W, false);

    assert_eq!(
        wrote(&mut state, "s1", RowChange::Archived(true)),
        Outcome::Noted
    );
    expected.archived = true;
    assert_eq!(state.row("s1"), Some(&expected));

    // Arranging changes only what is given.
    let pin = RowChange::Arranged {
        pinned: Some(true),
        section: None,
    };
    wrote(&mut state, "s1", pin);
    expected.pinned = true;
    assert_eq!(state.row("s1"), Some(&expected));
    let into_section = RowChange::Arranged {
        pinned: None,
        section: Some(Some(SECTION.into())),
    };
    wrote(&mut state, "s1", into_section);
    expected.section_id = Some(SECTION.into());
    assert_eq!(state.row("s1"), Some(&expected));
    let unpin = RowChange::Arranged {
        pinned: Some(false),
        section: None,
    };
    wrote(&mut state, "s1", unpin);
    expected.pinned = false;
    assert_eq!(state.row("s1"), Some(&expected));
    let out_of_it = RowChange::Arranged {
        pinned: Some(true),
        section: Some(None),
    };
    wrote(&mut state, "s1", out_of_it);
    (expected.pinned, expected.section_id) = (true, None);
    assert_eq!(state.row("s1"), Some(&expected));

    wrote(&mut state, "s1", RowChange::Deleting(Deleting::Running));
    expected.deleting = Some(Deleting::Running);
    assert_eq!(state.row("s1"), Some(&expected));
    wrote(&mut state, "s1", RowChange::Deleting(Deleting::Failed));
    expected.deleting = Some(Deleting::Failed);
    assert_eq!(state.row("s1"), Some(&expected));

    wrote(&mut state, "s1", RowChange::Mode("plan".into()));
    expected.mode = Some("plan".into());
    wrote(&mut state, "s1", RowChange::BackendId("b-1".into()));
    expected.backend_session_id = Some("b-1".into());
    wrote(&mut state, "s1", RowChange::LastActivity("t1".into()));
    expected.last_activity_at = Some("t1".into());
    assert_eq!(state.row("s1"), Some(&expected));

    // Each value Core gave the backend, with its own time.
    for (setting, value, at) in [
        (Setting::Model, "opus", "t2"),
        (Setting::Effort, "high", "t3"),
        (Setting::PermissionMode, "plan", "t4"),
    ] {
        let (value, at) = (value.to_string(), at.to_string());
        wrote(
            &mut state,
            "s1",
            RowChange::Requested { setting, value, at },
        );
    }
    (expected.model, expected.model_at) = (Some("opus".into()), Some("t2".into()));
    (expected.effort, expected.effort_at) = (Some("high".into()), Some("t3".into()));
    expected.permission_mode = Some("plan".into());
    expected.permission_mode_at = Some("t4".into());
    assert_eq!(state.row("s1"), Some(&expected));

    // A fork's link to what it was copied from.
    let mut stored = existing();
    let mut fork = row("s3", W, false);
    fork.forked_from = Some("b-0".into());
    fork.forked_through = Some("m-9".into());
    stored.sessions.push(fork);
    let mut forked = State::restored(stored);
    wrote(&mut forked, "s3", RowChange::ForkLinkCleared);
    assert_eq!(forked.row("s3"), Some(&row("s3", W, false)));

    wrote(&mut state, "s1", RowChange::Archived(false));
    expected.archived = false;
    assert_eq!(state.row("s1"), Some(&expected));
    assert_eq!(state.row("s2"), Some(&untouched));
}

// verifies: ARCH-CORE-R14
#[test]
fn a_write_for_a_session_that_is_gone_or_not_stored_yet_is_stale_and_keeps_nothing() {
    let mut state = world_with(&["s1"]);
    command(&mut state, create_session(W, "s2"));
    let before = state.clone();
    for session in ["s2", "s9"] {
        for change in writes(session) {
            assert_eq!(wrote(&mut state, session, change), Outcome::Stale);
        }
    }
    assert_eq!(state, before);

    // Gone since: the row does not come back with a late write.
    let gone = Report::SessionGone {
        session: "s1".into(),
    };
    report(&mut state, gone);
    let before = state.clone();
    assert_eq!(
        wrote(&mut state, "s1", RowChange::Archived(true)),
        Outcome::Stale
    );
    assert_eq!(state, before);
}

// verifies: PROTO-CORE-API-R39
#[test]
fn a_section_that_is_removed_leaves_every_row_that_named_it_and_no_other() {
    let mut stored = existing();
    for (id, section) in [("s1", Some(SECTION)), ("s2", Some("sec2")), ("s3", None)] {
        let mut row = row(id, W, false);
        row.section_id = section.map(str::to_string);
        row.pinned = true;
        stored.sessions.push(row);
    }
    let mut also = row("s4", ROOT, true);
    also.section_id = Some(SECTION.into());
    stored.sessions.push(also);
    stored.sections = vec![SECTION.into(), "sec2".into()];
    let mut state = State::restored(stored);

    let removed = Report::SectionRemoved {
        section: SECTION.into(),
    };
    // Said only by a removal under way: before one, it changes nothing.
    let before = state.clone();
    assert_eq!(report(&mut state, removed.clone()), Outcome::Stale);
    assert_eq!(state, before);
    command(&mut state, remove_section(SECTION));
    assert_eq!(report(&mut state, removed), Outcome::Noted);
    assert_eq!(state.section(SECTION), None);
    assert_eq!(state.section("sec2"), Some(SectionPhase::Present));
    let section = |state: &State, id: &str| state.row(id).and_then(|row| row.section_id.clone());
    assert_eq!(section(&state, "s1"), None);
    assert_eq!(section(&state, "s4"), None);
    assert_eq!(section(&state, "s2"), Some("sec2".into()));
    assert_eq!(section(&state, "s3"), None);
    // Nothing else about a row changes.
    assert!(state.row("s1").is_some_and(|row| row.pinned));
}

// -- Archive, arrange and sections (ARCH-CORE-R14) --------------------------

fn archive(session: &str) -> Command {
    Command::Archive {
        session: session.into(),
    }
}

fn arrange(session: &str, section: Option<&str>) -> Command {
    Command::Arrange {
        session: session.into(),
        section: section.map(str::to_string),
    }
}

fn remove_section(section: &str) -> Command {
    Command::RemoveSection {
        section: section.into(),
    }
}

/// A `session.send` from a sender that can start the session again.
fn send(session: &str) -> Command {
    Command::Send {
        session: session.into(),
        resumes: true,
    }
}

/// One from a sender that cannot.
fn send_to_running(session: &str) -> Command {
    Command::Send {
        session: session.into(),
        resumes: false,
    }
}

fn fork(session: &str) -> Command {
    Command::Fork {
        session: session.into(),
    }
}

fn forking(state: &State, session: &str) -> bool {
    state.session(session).is_some_and(|entry| entry.forking)
}

fn interrupt(session: &str) -> Command {
    Command::Interrupt {
        session: session.into(),
    }
}

fn await_send(session: &str, until: SendIs) -> Command {
    Command::AwaitSend {
        session: session.into(),
        until,
    }
}

fn stage(state: &mut State, session: &str, stage: SendStage) -> Outcome {
    let session = session.into();
    report(state, Report::SendStage { session, stage })
}

fn sending(state: &State, session: &str) -> Option<Sending> {
    state.session(session).and_then(|entry| entry.send)
}

fn ended(state: &mut State, operation: Operation) -> ersa_core::admission::Step {
    step(state, Input::Report(Report::Ended(operation)))
}

/// [`world_with`], and the section [`SECTION`].
fn world_with_section(sessions: &[&str]) -> State {
    let mut existing = existing();
    for session in sessions {
        existing.sessions.push(row(session, W, false));
    }
    existing.sections.push(SECTION.into());
    State::restored(existing)
}

// verifies: ARCH-CORE-R16
#[test]
fn two_archives_at_once_both_have_their_turn() {
    let mut state = world_with(&["s1"]);
    let archiving = admitted(Operation::Archive("s1".into()));
    assert_eq!(command(&mut state, archive("s1")), archiving);
    // The second waits for the first, as any exclusive operation does, and
    // so does anything else asked of the session meanwhile.
    assert_eq!(command(&mut state, archive("s1")), Outcome::Held);
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);
    wrote(&mut state, "s1", RowChange::Archived(true));

    // The first ends: the second has its turn, and finds the session
    // archived.
    let first = ended(&mut state, Operation::Archive("s1".into()));
    assert_eq!(first.released, [(0, archiving.clone())]);
    assert!(state.row("s1").is_some_and(|row| row.archived));
    let second = ended(&mut state, Operation::Archive("s1".into()));
    let other = admitted(Operation::Exclusive("s1".into()));
    assert_eq!(second.released, [(0, other)]);
    ended(&mut state, Operation::Exclusive("s1".into()));
    assert!(!exclusive(&state, "s1"));
    assert!(state.held().is_empty());

    // An archive of a session that is gone is refused.
    assert_eq!(
        command(&mut state, archive("s9")),
        Outcome::Refused(Refusal::NoSession)
    );
}

// verifies: ARCH-CORE-R16
#[test]
fn an_operation_s_end_ends_only_its_own_kind() {
    let mut state = world_with(&["s1"]);
    command(&mut state, archive("s1"));
    let before = state.clone();
    for other in [
        Operation::Exclusive("s1".into()),
        Operation::Arrange("s1".into()),
    ] {
        assert_eq!(ended(&mut state, other).outcome, Outcome::Stale);
    }
    assert_eq!(state, before);
    assert_eq!(
        ended(&mut state, Operation::Archive("s1".into())).outcome,
        Outcome::Noted
    );

    command(&mut state, arrange("s1", None));
    let before = state.clone();
    for other in [
        Operation::Exclusive("s1".into()),
        Operation::Archive("s1".into()),
    ] {
        assert_eq!(ended(&mut state, other).outcome, Outcome::Stale);
    }
    assert_eq!(state, before);
    assert_eq!(
        ended(&mut state, Operation::Arrange("s1".into())).outcome,
        Outcome::Noted
    );
    assert!(!exclusive(&state, "s1"));
}

// verifies: PROTO-CORE-API-R85
#[test]
fn a_send_is_refused_while_an_archive_is_under_way_or_waiting_its_turn() {
    let mut state = world_with(&["s1", "s2"]);
    let sent = |state: &mut State, session: &str| {
        let admitted = command(state, send(session));
        ended(state, Operation::Send(session.into()));
        admitted
    };
    let may_go = admitted(Operation::Send("s1".into()));
    assert_eq!(sent(&mut state, "s1"), may_go);

    // An archive waiting its turn, here behind a view being opened, already
    // refuses a send: none starts only to be stopped by the archive.
    command(&mut state, begin_exclusive("s1"));
    assert_eq!(sent(&mut state, "s1"), may_go);
    assert_eq!(command(&mut state, archive("s1")), Outcome::Held);
    assert!(state.being_archived("s1"));
    let refused = Outcome::Refused(Refusal::BeingArchived);
    assert_eq!(command(&mut state, send("s1")), refused);
    assert_eq!(sending(&state, "s1"), None);
    // Another session's sends go on.
    assert_eq!(
        sent(&mut state, "s2"),
        admitted(Operation::Send("s2".into()))
    );
    assert!(!state.being_archived("s2"));

    // Under way: still refused.
    let opened = ended(&mut state, Operation::Exclusive("s1".into()));
    assert_eq!(
        opened.released,
        [(0, admitted(Operation::Archive("s1".into())))]
    );
    assert_eq!(command(&mut state, send("s1")), refused);

    // Over, and not archived after all: a send may go.
    ended(&mut state, Operation::Archive("s1".into()));
    assert!(!state.being_archived("s1"));
    assert_eq!(sent(&mut state, "s1"), may_go);

    // An unarchive, or any other exclusive operation, refuses no send.
    command(&mut state, begin_exclusive("s1"));
    assert_eq!(sent(&mut state, "s1"), may_go);
}

// -- The send slot (ARCH-CORE-R14) ------------------------------------------

// verifies: ARCH-CORE-R14
// verifies: ARCH-CORE-R16
#[test]
fn a_second_send_is_refused_while_one_holds_the_slot() {
    let mut state = world_with(&["s1", "s2"]);
    let first = admitted(Operation::Send("s1".into()));
    assert_eq!(command(&mut state, send("s1")), first);
    let checking = Sending {
        stage: SendStage::Checking,
        canceled: false,
    };
    assert_eq!(sending(&state, "s1"), Some(checking));

    // One at a time: refused, never queued, and nothing changes.
    let before = state.clone();
    let in_progress = Outcome::Refused(Refusal::SendInProgress);
    assert_eq!(command(&mut state, send("s1")), in_progress);
    assert_eq!(state, before);
    // Another session has a slot of its own.
    assert_eq!(
        command(&mut state, send("s2")),
        admitted(Operation::Send("s2".into()))
    );

    // Through its stages, each once and in order.
    assert_eq!(stage(&mut state, "s1", SendStage::Checking), Outcome::Stale);
    assert_eq!(stage(&mut state, "s1", SendStage::Starting), Outcome::Noted);
    assert_eq!(stage(&mut state, "s1", SendStage::Starting), Outcome::Stale);
    assert_eq!(command(&mut state, send("s1")), in_progress);
    assert_eq!(
        stage(&mut state, "s1", SendStage::HandingOver),
        Outcome::Noted
    );
    assert_eq!(stage(&mut state, "s1", SendStage::Starting), Outcome::Stale);
    assert_eq!(
        stage(&mut state, "s1", SendStage::HandingOver),
        Outcome::Stale
    );
    assert_eq!(command(&mut state, send("s1")), in_progress);

    // However it ends, the slot is free, and only once.
    assert_eq!(
        ended(&mut state, Operation::Send("s1".into())).outcome,
        Outcome::Noted
    );
    assert_eq!(sending(&state, "s1"), None);
    assert_eq!(
        ended(&mut state, Operation::Send("s1".into())).outcome,
        Outcome::Stale
    );
    assert_eq!(command(&mut state, send("s1")), first);
    // A stage reported for a session with no send is stale.
    ended(&mut state, Operation::Send("s1".into()));
    assert_eq!(stage(&mut state, "s1", SendStage::Starting), Outcome::Stale);
}

// verifies: PROTO-CORE-API-R25
#[test]
fn an_interrupt_cancels_a_send_or_finds_it_handed_over_never_both() {
    let no_send = Outcome::Refused(Refusal::NoSendToCancel);

    // The interrupt first: the send is cancelled, and is not handed over.
    for started in [false, true] {
        let mut state = world_with(&["s1"]);
        assert_eq!(command(&mut state, interrupt("s1")), no_send);
        command(&mut state, send("s1"));
        if started {
            stage(&mut state, "s1", SendStage::Starting);
        }
        assert_eq!(command(&mut state, interrupt("s1")), Outcome::Done);
        assert!(sending(&state, "s1").is_some_and(|send| send.canceled));
        let before = state.clone();
        assert_eq!(
            stage(&mut state, "s1", SendStage::HandingOver),
            Outcome::Stale
        );
        assert_eq!(state, before);
        // A second interrupt of it is the same answer.
        assert_eq!(command(&mut state, interrupt("s1")), Outcome::Done);
        // Stopped, its slot is free, and a new send is not cancelled.
        ended(&mut state, Operation::Send("s1".into()));
        command(&mut state, send("s1"));
        assert!(sending(&state, "s1").is_some_and(|send| !send.canceled));
    }

    // The hand-over first: the interrupt finds no send to cancel, and goes
    // on to the turn.
    let mut state = world_with(&["s1"]);
    command(&mut state, send("s1"));
    assert_eq!(
        stage(&mut state, "s1", SendStage::HandingOver),
        Outcome::Noted
    );
    let before = state.clone();
    assert_eq!(command(&mut state, interrupt("s1")), no_send);
    assert_eq!(state, before);
    assert!(sending(&state, "s1").is_some_and(|send| !send.canceled));

    // A session that is gone has none.
    assert_eq!(
        command(&mut state, interrupt("s9")),
        Outcome::Refused(Refusal::NoSession)
    );
}

// verifies: ARCH-CORE-R14
#[test]
fn a_send_is_told_why_it_may_not_go_in_one_order() {
    let refused = |state: &mut State, command_: Command| match command(state, command_) {
        Outcome::Refused(refusal) => refusal,
        other => panic!("not refused: {other:?}"),
    };
    // Every reason at once, taken away one at a time.
    let mut state = world_with(&["s1"]);
    command(&mut state, delete_session("s1"));
    wrote(&mut state, "s1", RowChange::Deleting(Deleting::Failed));
    wrote(&mut state, "s1", RowChange::Archived(true));
    command(&mut state, begin_exclusive("s1"));
    command(&mut state, archive("s1"));
    command(&mut state, send_to_running("s1"));
    assert_eq!(sending(&state, "s1"), None);

    // Its own delete has its workspace.
    assert_eq!(refused(&mut state, send("s1")), Refusal::WorkspaceHeld);
    ended(&mut state, Operation::DeleteSession("s1".into()));
    // It is marked deleting.
    assert_eq!(refused(&mut state, send("s1")), Refusal::SessionDeleting);

    // Without the mark: being archived comes before archived, and archived
    // before not running, so nothing resumes an archived session.
    let mut state = world_with(&["s1"]);
    wrote(&mut state, "s1", RowChange::Archived(true));
    command(&mut state, begin_exclusive("s1"));
    command(&mut state, archive("s1"));
    assert_eq!(
        refused(&mut state, send_to_running("s1")),
        Refusal::BeingArchived
    );
    ended(&mut state, Operation::Exclusive("s1".into()));
    ended(&mut state, Operation::Archive("s1".into()));
    assert_eq!(
        refused(&mut state, send_to_running("s1")),
        Refusal::Archived
    );
    assert_eq!(refused(&mut state, send("s1")), Refusal::Archived);
    wrote(&mut state, "s1", RowChange::Archived(false));

    // Stopped: only a sender that can start it again may send.
    assert_eq!(
        refused(&mut state, send_to_running("s1")),
        Refusal::NotRunning
    );
    // Not running comes before a send already under way.
    command(&mut state, send("s1"));
    assert_eq!(
        refused(&mut state, send_to_running("s1")),
        Refusal::NotRunning
    );
    assert_eq!(refused(&mut state, send("s1")), Refusal::SendInProgress);
    ended(&mut state, Operation::Send("s1".into()));

    // Running, or ending (the start the send asks for waits for that): any
    // sender.
    command(&mut state, launch("s1"));
    assert_eq!(
        command(&mut state, send_to_running("s1")),
        admitted(Operation::Send("s1".into()))
    );
    ended(&mut state, Operation::Send("s1".into()));
    report(&mut state, run_ending("s1"));
    assert_eq!(
        command(&mut state, send_to_running("s1")),
        admitted(Operation::Send("s1".into()))
    );

    // A session that is gone, or whose row is not stored yet.
    assert_eq!(refused(&mut state, send("s9")), Refusal::NoSession);
    command(&mut state, create_session(W, "s2"));
    assert_eq!(refused(&mut state, send("s2")), Refusal::NoSession);
    // Another session's delete in the same workspace refuses no send here.
    let mut state = world_with(&["s1", "s2"]);
    command(&mut state, delete_session("s2"));
    assert_eq!(
        command(&mut state, send("s1")),
        admitted(Operation::Send("s1".into()))
    );
}

// verifies: PROTO-CORE-API-R90
#[test]
fn a_waiting_send_s_reporter_hears_of_a_send_that_takes_the_slot_and_of_its_end() {
    let mut state = world_with(&["s1", "s2"]);
    // No send has the slot: "free" is so already, "taken" waits.
    assert_eq!(
        command(&mut state, await_send("s1", SendIs::Free)),
        Outcome::Done
    );
    assert_eq!(
        command(&mut state, await_send("s1", SendIs::Taken)),
        Outcome::Held
    );
    // Another session's send is not this one's.
    command(&mut state, send("s2"));
    assert_eq!(state.held().len(), 1);

    // A send takes the slot: the reporter hears of it in that step, however
    // short the send turns out to be.
    let taken = step(&mut state, Input::Command(send("s1")));
    assert_eq!(taken.outcome, admitted(Operation::Send("s1".into())));
    assert_eq!(taken.released, [(0, Outcome::Done)]);
    assert_eq!(
        command(&mut state, await_send("s1", SendIs::Taken)),
        Outcome::Done
    );

    // And then waits for it to end, however it ends.
    assert_eq!(
        command(&mut state, await_send("s1", SendIs::Free)),
        Outcome::Held
    );
    stage(&mut state, "s1", SendStage::HandingOver);
    assert_eq!(state.held().len(), 1);
    let freed = ended(&mut state, Operation::Send("s1".into()));
    assert_eq!(freed.released, [(0, Outcome::Done)]);
    assert!(state.held().is_empty());

    // A session that goes while its reporter waits: there is nothing to
    // report for.
    command(&mut state, await_send("s1", SendIs::Taken));
    let gone = Report::SessionGone {
        session: "s1".into(),
    };
    let went = step(&mut state, Input::Report(gone));
    assert_eq!(went.released, [(0, Outcome::Refused(Refusal::NoSession))]);
}

// verifies: PROTO-CORE-API-R25
#[test]
fn a_send_hears_in_admission_that_it_was_cancelled() {
    let cancelled = || await_send("s1", SendIs::Cancelled);
    let no_send = Outcome::Refused(Refusal::NoSendToCancel);
    let mut state = world_with(&["s1"]);
    // No send: nothing to hear of.
    assert_eq!(command(&mut state, cancelled()), no_send);

    // The send waits to hear, and an interrupt tells it in its own step.
    command(&mut state, send("s1"));
    assert_eq!(command(&mut state, cancelled()), Outcome::Held);
    let interrupted = step(&mut state, Input::Command(interrupt("s1")));
    assert_eq!(interrupted.outcome, Outcome::Done);
    assert_eq!(interrupted.released, [(0, Outcome::Done)]);
    // One that asks after it was cancelled is told at once.
    assert_eq!(command(&mut state, cancelled()), Outcome::Done);
    ended(&mut state, Operation::Send("s1".into()));

    // A send that ends without being cancelled: its wait ends with it, and
    // a later send's cancel is not heard by it.
    command(&mut state, send("s1"));
    assert_eq!(command(&mut state, cancelled()), Outcome::Held);
    let over = ended(&mut state, Operation::Send("s1".into()));
    assert_eq!(over.released, [(0, no_send.clone())]);
    command(&mut state, send("s1"));
    let interrupted = step(&mut state, Input::Command(interrupt("s1")));
    assert!(interrupted.released.is_empty());

    // An interrupt that finds the send handed over tells nobody.
    let mut state = world_with(&["s1"]);
    command(&mut state, send("s1"));
    command(&mut state, cancelled());
    stage(&mut state, "s1", SendStage::HandingOver);
    let late = step(&mut state, Input::Command(interrupt("s1")));
    assert_eq!(late.outcome, no_send);
    assert!(late.released.is_empty());
    assert_eq!(state.held().len(), 1);
}

// -- A fork's mark on its original (ARCH-CORE-R14) --------------------------

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_fork_starting_refuses_a_send_a_delete_and_a_second_fork_and_nothing_else() {
    let mut state = world_with(&["s1", "s2"]);
    command(&mut state, launch("s1"));
    let marked = admitted(Operation::Fork("s1".into()));
    assert_eq!(command(&mut state, fork("s1")), marked);
    assert!(forking(&state, "s1"));

    // No message, no delete, no second fork, and none of them changes
    // anything: a delete refused for the fork holds no workspace.
    let before = state.clone();
    let being_forked = Outcome::Refused(Refusal::BeingForked);
    assert_eq!(command(&mut state, send("s1")), being_forked);
    assert_eq!(command(&mut state, send_to_running("s1")), being_forked);
    assert_eq!(command(&mut state, delete_session("s1")), being_forked);
    assert_eq!(
        command(&mut state, fork("s1")),
        Outcome::Refused(Refusal::Forking)
    );
    assert_eq!(state, before);
    assert!(!state.workspace_held(W));

    // Another session is not marked: it takes a message and is forked.
    assert_eq!(
        command(&mut state, send("s2")),
        admitted(Operation::Send("s2".into()))
    );
    ended(&mut state, Operation::Send("s2".into()));
    assert_eq!(
        command(&mut state, fork("s2")),
        admitted(Operation::Fork("s2".into()))
    );
    ended(&mut state, Operation::Fork("s2".into()));

    // The original is stopped, archived and arranged meanwhile: the mark is
    // not its exclusive operation.
    for begun in [begin_exclusive("s1"), archive("s1"), arrange("s1", None)] {
        let operation = match command(&mut state, begun) {
            Outcome::Admitted(operation) => operation,
            other => panic!("not admitted: {other:?}"),
        };
        assert_eq!(ended(&mut state, operation).outcome, Outcome::Noted);
    }
    assert!(forking(&state, "s1"));

    // The fork has started, or never will: the mark goes, and only once.
    assert_eq!(
        ended(&mut state, Operation::Fork("s1".into())).outcome,
        Outcome::Noted
    );
    assert!(!forking(&state, "s1"));
    assert_eq!(
        ended(&mut state, Operation::Fork("s1".into())).outcome,
        Outcome::Stale
    );
    assert_eq!(
        command(&mut state, send_to_running("s1")),
        admitted(Operation::Send("s1".into()))
    );
}

// verifies: FLOW-FORK-SESSION-R03
#[test]
fn a_fork_is_told_why_it_may_not_begin_in_one_order() {
    let refused = |state: &mut State| match command(state, fork("s1")) {
        Outcome::Refused(refusal) => refusal,
        other => panic!("not refused: {other:?}"),
    };
    // Every reason at once, taken away one at a time.
    let mut state = world_with(&["s1", "s2"]);
    command(&mut state, fork("s1"));
    ended(&mut state, Operation::Fork("s1".into()));
    command(&mut state, send("s1"));
    wrote(&mut state, "s1", RowChange::Deleting(Deleting::Failed));
    command(&mut state, delete_session("s1"));
    let before = state.clone();

    // Its own delete has its workspace.
    assert_eq!(refused(&mut state), Refusal::WorkspaceHeld);
    assert_eq!(state, before);
    ended(&mut state, Operation::DeleteSession("s1".into()));
    // It is marked deleting: said before a hold of its workspace that is
    // not its own delete's.
    command(&mut state, delete_session("s2"));
    assert_eq!(refused(&mut state), Refusal::SessionDeleting);

    // Something else holds its workspace, here another session's delete.
    let mut state = world_with(&["s1", "s2"]);
    command(&mut state, send("s1"));
    command(&mut state, delete_session("s2"));
    assert_eq!(refused(&mut state), Refusal::WorkspaceHeld);
    assert!(!forking(&state, "s1"));

    // A message is on its way: the turn it starts would be copied part way.
    let mut state = world_with(&["s1"]);
    command(&mut state, send("s1"));
    assert_eq!(refused(&mut state), Refusal::SendInProgress);
    assert!(!forking(&state, "s1"));
    ended(&mut state, Operation::Send("s1".into()));
    assert_eq!(
        command(&mut state, fork("s1")),
        admitted(Operation::Fork("s1".into()))
    );

    // A session that is gone, or whose row is not stored yet.
    assert_eq!(
        command(&mut state, fork("s9")),
        Outcome::Refused(Refusal::NoSession)
    );
    command(&mut state, create_session(W, "s2"));
    assert_eq!(
        command(&mut state, fork("s2")),
        Outcome::Refused(Refusal::NoSession)
    );
}

// verifies: PROTO-CORE-API-R41
#[test]
fn an_original_is_removed_while_a_fork_of_it_is_starting() {
    let mut state = world_with(&["s1"]);
    command(&mut state, fork("s1"));
    assert_eq!(
        command(&mut state, remove_session("s1")),
        admitted(Operation::RemoveSession("s1".into()))
    );
    assert_eq!(
        command(&mut state, begin_removal("s1")),
        admitted(Operation::Exclusive("s1".into()))
    );
    let gone = Report::SessionGone {
        session: "s1".into(),
    };
    assert_eq!(report(&mut state, gone), Outcome::Noted);
    // The fork's holders let go later, of nothing; and a fork that asks
    // again finds no session.
    let before = state.clone();
    assert_eq!(
        ended(&mut state, Operation::Fork("s1".into())).outcome,
        Outcome::Stale
    );
    assert_eq!(
        command(&mut state, fork("s1")),
        Outcome::Refused(Refusal::NoSession)
    );
    assert_eq!(state, before);
}

// verifies: PROTO-CORE-API-R41
#[test]
fn a_session_is_removed_while_a_message_to_it_is_still_copying() {
    // A send to a stopped session, still copying its attachments.
    let mut state = world_with(&["s1"]);
    command(&mut state, send("s1"));

    // A removal does not wait for it.
    assert_eq!(
        command(&mut state, remove_session("s1")),
        admitted(Operation::RemoveSession("s1".into()))
    );
    assert_eq!(
        command(&mut state, begin_removal("s1")),
        admitted(Operation::Exclusive("s1".into()))
    );
    let gone = Report::SessionGone {
        session: "s1".into(),
    };
    assert_eq!(report(&mut state, gone), Outcome::Noted);
    assert_eq!(sending(&state, "s1"), None);

    // Every later step of the send finds no session: it starts nothing,
    // hands nothing over, and its end frees nothing.
    let before = state.clone();
    assert_eq!(stage(&mut state, "s1", SendStage::Starting), Outcome::Stale);
    assert_eq!(
        command(&mut state, begin_start("s1")),
        Outcome::Refused(Refusal::NoSession)
    );
    assert_eq!(
        stage(&mut state, "s1", SendStage::HandingOver),
        Outcome::Stale
    );
    assert_eq!(
        ended(&mut state, Operation::Send("s1".into())).outcome,
        Outcome::Stale
    );
    assert_eq!(state, before);
}

// verifies: PROTO-CORE-API-R39
#[test]
fn a_section_s_removal_and_an_arrangement_into_it_land_in_one_order() {
    let section = |state: &State, id: &str| state.row(id).and_then(|row| row.section_id.clone());
    let into = || RowChange::Arranged {
        pinned: None,
        section: Some(Some(SECTION.into())),
    };
    let removed = || Report::SectionRemoved {
        section: SECTION.into(),
    };

    // The arrangement first: the removal waits for it, and then takes the
    // session out of the section with the rest.
    let mut state = world_with_section(&["s1", "s2"]);
    let arranging = admitted(Operation::Arrange("s1".into()));
    assert_eq!(command(&mut state, arrange("s1", Some(SECTION))), arranging);
    assert_eq!(command(&mut state, remove_section(SECTION)), Outcome::Held);
    assert_eq!(state.section(SECTION), Some(SectionPhase::Present));
    wrote(&mut state, "s1", into());
    let arranged = ended(&mut state, Operation::Arrange("s1".into()));
    let removing = admitted(Operation::RemoveSection(SECTION.into()));
    assert_eq!(arranged.released, [(0, removing.clone())]);
    assert_eq!(state.section(SECTION), Some(SectionPhase::Removing));
    report(&mut state, removed());
    assert_eq!(section(&state, "s1"), None);
    assert_eq!(state.section(SECTION), None);

    // The removal first: an arrangement into the section waits for it, and
    // then finds no such section. One that does not move its session into
    // that section does not wait.
    let mut state = world_with_section(&["s1", "s2"]);
    assert_eq!(command(&mut state, remove_section(SECTION)), removing);
    assert_eq!(
        command(&mut state, arrange("s1", Some(SECTION))),
        Outcome::Held
    );
    assert!(!exclusive(&state, "s1"));
    assert_eq!(
        command(&mut state, arrange("s2", None)),
        admitted(Operation::Arrange("s2".into()))
    );
    // A second removal of it waits too.
    assert_eq!(command(&mut state, remove_section(SECTION)), Outcome::Held);
    let gone = step(&mut state, Input::Report(removed()));
    let no_section = Outcome::Refused(Refusal::NoSection);
    assert_eq!(
        gone.released,
        [(0, no_section.clone()), (1, no_section.clone())]
    );
    assert_eq!(section(&state, "s1"), None);
    assert!(!exclusive(&state, "s1"));

    // A removal that fails leaves the section, and what waited goes on.
    let mut state = world_with_section(&["s1"]);
    command(&mut state, remove_section(SECTION));
    command(&mut state, arrange("s1", Some(SECTION)));
    let failed = ended(&mut state, Operation::RemoveSection(SECTION.into()));
    assert_eq!(failed.released, [(0, arranging)]);
    assert_eq!(state.section(SECTION), Some(SectionPhase::Present));
}

// verifies: PROTO-CORE-API-R39
#[test]
fn a_section_that_does_not_exist_is_neither_arranged_into_nor_removed() {
    let mut state = world_with_section(&["s1"]);
    let before = state.clone();
    let no_section = Outcome::Refused(Refusal::NoSection);
    assert_eq!(command(&mut state, arrange("s1", Some("sec9"))), no_section);
    assert_eq!(command(&mut state, remove_section("sec9")), no_section);
    // A session that is gone is said first.
    assert_eq!(
        command(&mut state, arrange("s9", Some("sec9"))),
        Outcome::Refused(Refusal::NoSession)
    );
    assert_eq!(state, before);

    // A section is there once it is stored, and only once.
    let stored = || Report::SectionStored {
        section: "sec9".into(),
    };
    assert_eq!(report(&mut state, stored()), Outcome::Noted);
    assert_eq!(state.section("sec9"), Some(SectionPhase::Present));
    assert_eq!(report(&mut state, stored()), Outcome::Stale);
    assert_eq!(
        command(&mut state, arrange("s1", Some("sec9"))),
        admitted(Operation::Arrange("s1".into()))
    );
}

// verifies: PROTO-CORE-API-R39
#[test]
fn a_row_stored_in_a_section_that_went_meanwhile_is_kept_in_none() {
    // A fork is stored in its original's section. A removal of that section
    // that lands between the store and the report took the row out of it
    // there, so admission keeps it in none.
    let mut state = world_with_section(&["s1"]);
    command(&mut state, create_session(W, "s2"));
    command(&mut state, remove_section(SECTION));
    report(
        &mut state,
        Report::SectionRemoved {
            section: SECTION.into(),
        },
    );
    let mut fork = row("s2", W, false);
    fork.section_id = Some(SECTION.into());
    let stored = Report::SessionStored {
        row: Box::new(fork.clone()),
    };
    assert_eq!(report(&mut state, stored), Outcome::Noted);
    assert_eq!(state.row("s2"), Some(&row("s2", W, false)));

    // In a section that is there, it stays.
    let mut state = world_with_section(&["s1"]);
    command(&mut state, create_session(W, "s2"));
    let stored = Report::SessionStored {
        row: Box::new(fork.clone()),
    };
    report(&mut state, stored);
    assert_eq!(state.row("s2"), Some(&fork));
}

/// [`world`] with `sessions` Ersa started, stored, in the isolated
/// workspace.
fn world_with(sessions: &[&str]) -> State {
    let mut existing = existing();
    for session in sessions {
        existing.sessions.push(row(session, W, false));
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
    let stored = step(&mut state, Input::Report(session_stored("s2")));
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
    found_in_isolated.sessions.push(row("s1", W, true));
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
    found_in_root.sessions.push(row("s1", ROOT, true));
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

fn exclusive(state: &State, session: &str) -> bool {
    state
        .session(session)
        .is_some_and(|s| s.exclusive.is_some())
}

// verifies: ARCH-CORE-R14
#[test]
fn a_second_exclusive_operation_waits_and_is_admitted_when_the_first_ends() {
    let mut state = world_with(&["s1", "s2"]);
    assert_eq!(
        command(&mut state, begin_exclusive("s1")),
        admitted(Operation::Exclusive("s1".into()))
    );
    assert!(exclusive(&state, "s1"));
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
    assert!(exclusive(&state, "s1"));
    assert_eq!(state.held().len(), 1);

    // One asked of a session admission does not have is refused.
    assert_eq!(
        command(&mut state, begin_exclusive("s9")),
        Outcome::Refused(Refusal::NoSession)
    );
}

// verifies: ARCH-CORE-R14
#[test]
fn a_removal_takes_the_exclusive_operation_as_a_second_step() {
    // A removal asked for marks the session, so a delete of it waits, and
    // does not yet have the session's exclusive operation: while it waits
    // for a run that is winding down, another operation goes on.
    let mut state = world_with(&["s1"]);
    command(&mut state, remove_session("s1"));
    assert!(!exclusive(&state, "s1"));
    assert_eq!(command(&mut state, delete_session("s1")), Outcome::Held);
    assert_eq!(
        command(&mut state, begin_exclusive("s1")),
        admitted(Operation::Exclusive("s1".into()))
    );
    // The removal's own, asked for next, waits for that one to end.
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        ended.released,
        [(1, admitted(Operation::Exclusive("s1".into())))]
    );

    // The session goes: the delete that waited finds none, and so does an
    // operation asked for behind the removal's.
    command(&mut state, begin_exclusive("s1"));
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
}

#[test]
fn an_operation_asked_of_a_session_still_being_stored_waits_for_its_row() {
    let mut state = world();
    command(&mut state, create_session(W, "s1"));
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);
    let stored = step(&mut state, Input::Report(session_stored("s1")));
    assert_eq!(
        stored.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );
}

// -- A session's run, and shutdown -------------------------------------------

fn begin_start(session: &str) -> Command {
    Command::BeginStart {
        session: session.into(),
    }
}

fn begin_removal(session: &str) -> Command {
    Command::BeginRemoval {
        session: session.into(),
    }
}

fn launch(session: &str) -> Command {
    Command::Launch {
        session: session.into(),
    }
}

fn await_run_end(session: &str) -> Command {
    Command::AwaitRunEnd {
        session: session.into(),
    }
}

fn run_of(state: &State, session: &str) -> Option<Run> {
    state.session(session).map(|s| s.run)
}

/// The end of `session`'s run: Core has let go of it.
fn run_ended(session: &str) -> Input {
    Input::Report(Report::Ended(Operation::Run(session.into())))
}

fn run_ending(session: &str) -> Report {
    Report::RunEnding {
        session: session.into(),
    }
}

// verifies: ARCH-CORE-R14
#[test]
fn nothing_starts_once_core_is_closing() {
    let mut state = world_with(&["s1"]);
    // A start admitted before shutdown, about to launch.
    command(&mut state, begin_start("s1"));
    assert_eq!(report(&mut state, Report::Closing), Outcome::Noted);
    assert!(state.closing());

    // Its launch, reported after shutdown, is refused: no backend starts.
    assert_eq!(
        command(&mut state, launch("s1")),
        Outcome::Refused(Refusal::Closing)
    );
    assert_eq!(run_of(&state, "s1"), Some(Run::Stopped));
    // So are a new session and, once the first start has ended, another.
    assert_eq!(
        command(&mut state, create_session(W, "s2")),
        Outcome::Refused(Refusal::Closing)
    );
    assert_eq!(command(&mut state, begin_start("s1")), Outcome::Held);
    let ended = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(ended.released, [(0, Outcome::Refused(Refusal::Closing))]);

    // Before shutdown, the launch begins the run.
    let mut state = world_with(&["s1"]);
    command(&mut state, begin_start("s1"));
    assert_eq!(
        command(&mut state, launch("s1")),
        admitted(Operation::Run("s1".into()))
    );
    assert_eq!(run_of(&state, "s1"), Some(Run::Running));
}

/// `s1`, stored in the isolated workspace, with a run that was announced
/// ended and that Core has not let go of.
fn winding_down() -> State {
    let mut state = world_with(&["s1"]);
    command(&mut state, launch("s1"));
    assert_eq!(report(&mut state, run_ending("s1")), Outcome::Noted);
    assert_eq!(run_of(&state, "s1"), Some(Run::Ending));
    state
}

// verifies: PROTO-CORE-API-R41
#[test]
fn a_removal_behind_a_run_that_is_ending_waits_for_it_or_gives_up() {
    // A client saw the session end and removes it before Core has let go
    // of the run: the removal waits, and goes on when the run has ended.
    let mut state = winding_down();
    command(&mut state, remove_session("s1"));
    assert_eq!(command(&mut state, begin_removal("s1")), Outcome::Held);
    // While it waits, another operation on the session is not held up.
    assert_eq!(
        command(&mut state, begin_exclusive("s1")),
        admitted(Operation::Exclusive("s1".into()))
    );
    step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    let ended = step(&mut state, run_ended("s1"));
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );

    // Its time limit passes first: it is answered "gave up", and nothing
    // else changes.
    let mut state = winding_down();
    command(&mut state, remove_session("s1"));
    command(&mut state, begin_removal("s1"));
    let before = state.clone();
    let gave_up = step(&mut state, Input::Report(Report::GaveUp { place: 0 }));
    assert_eq!(gave_up.outcome, Outcome::Noted);
    assert_eq!(gave_up.released, [(0, Outcome::Refused(Refusal::GaveUp))]);
    assert!(state.held().is_empty());
    assert_eq!(
        state.sessions().collect::<Vec<_>>(),
        before.sessions().collect::<Vec<_>>()
    );
    // A limit reported for nothing that waits changes nothing.
    assert_eq!(
        report(&mut state, Report::GaveUp { place: 0 }),
        Outcome::Stale
    );

    // A session that runs is not removed: it is stopped first.
    let mut state = world_with(&["s1"]);
    command(&mut state, launch("s1"));
    command(&mut state, remove_session("s1"));
    assert_eq!(
        command(&mut state, begin_removal("s1")),
        Outcome::Refused(Refusal::Running)
    );
}

// verifies: FLOW-RESUME-SESSION-R02
#[test]
fn a_start_behind_a_run_that_is_ending_waits_for_it_or_gives_up() {
    // A message sent just after the backend exited resumes the session
    // rather than finding it still running.
    let mut state = winding_down();
    assert_eq!(command(&mut state, begin_start("s1")), Outcome::Held);
    let ended = step(&mut state, run_ended("s1"));
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );
    assert_eq!(
        command(&mut state, launch("s1")),
        admitted(Operation::Run("s1".into()))
    );

    let mut state = winding_down();
    command(&mut state, begin_start("s1"));
    let gave_up = step(&mut state, Input::Report(Report::GaveUp { place: 0 }));
    assert_eq!(gave_up.released, [(0, Outcome::Refused(Refusal::GaveUp))]);

    // A session that runs needs no start.
    let mut state = world_with(&["s1"]);
    command(&mut state, launch("s1"));
    assert_eq!(
        command(&mut state, begin_start("s1")),
        Outcome::Refused(Refusal::Running)
    );
}

// verifies: PROTO-CORE-API-R90
#[test]
fn a_send_asks_whether_the_session_runs_and_waits_for_a_run_that_is_ending() {
    // A message sent just after the session ended waits for Core to let go
    // of the run, to know that it resumes; it begins nothing by asking.
    let mut state = winding_down();
    let before = state.clone();
    assert_eq!(command(&mut state, await_run_end("s1")), Outcome::Held);
    let ended = step(&mut state, run_ended("s1"));
    assert_eq!(ended.released, [(0, Outcome::Done)]);
    assert!(!exclusive(&state, "s1"));

    // Its time limit passes first.
    let mut state = before;
    command(&mut state, await_run_end("s1"));
    let gave_up = step(&mut state, Input::Report(Report::GaveUp { place: 0 }));
    assert_eq!(gave_up.released, [(0, Outcome::Refused(Refusal::GaveUp))]);

    // Running: so it says. Stopped: done, and nothing changed.
    let mut state = world_with(&["s1"]);
    assert_eq!(command(&mut state, await_run_end("s1")), Outcome::Done);
    command(&mut state, launch("s1"));
    let running = state.clone();
    assert_eq!(
        command(&mut state, await_run_end("s1")),
        Outcome::Refused(Refusal::Running)
    );
    assert_eq!(state, running);
    assert_eq!(
        command(&mut state, await_run_end("s9")),
        Outcome::Refused(Refusal::NoSession)
    );
}

// verifies: ARCH-CORE-R17
#[test]
fn a_time_limit_answers_the_command_at_its_place_and_no_other_like_it() {
    // Two resumes of one session wait behind its run that is ending: the
    // same command twice. The second's limit passes: it alone is answered,
    // and the first still waits, and is admitted when the run ends.
    let mut state = winding_down();
    command(&mut state, begin_start("s1"));
    command(&mut state, begin_start("s1"));
    let gave_up = step(&mut state, Input::Report(Report::GaveUp { place: 1 }));
    assert_eq!(gave_up.released, [(1, Outcome::Refused(Refusal::GaveUp))]);
    assert_eq!(state.held().len(), 1);
    let ended = step(&mut state, run_ended("s1"));
    assert_eq!(
        ended.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );
    // A place nothing waits at: nothing changes.
    assert_eq!(
        report(&mut state, Report::GaveUp { place: 0 }),
        Outcome::Stale
    );
}

// verifies: ARCH-CORE-R17
#[test]
fn a_stop_waits_for_a_start_and_a_start_and_a_view_opening_wait_for_each_other() {
    // A stop asked for while a resume is under way waits until the session
    // is live, then has something to stop.
    let mut state = world_with(&["s1"]);
    command(&mut state, begin_start("s1"));
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);
    command(&mut state, launch("s1"));
    let started = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        started.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );
    assert_eq!(run_of(&state, "s1"), Some(Run::Running));

    // A resume asked for while a view is being opened waits for it.
    let mut state = world_with(&["s1"]);
    command(&mut state, begin_exclusive("s1"));
    assert_eq!(command(&mut state, begin_start("s1")), Outcome::Held);
    let opened = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        opened.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );

    // And a view asked for while a resume is under way waits for the start.
    let mut state = world_with(&["s1"]);
    command(&mut state, begin_start("s1"));
    assert_eq!(command(&mut state, begin_exclusive("s1")), Outcome::Held);

    // A resume and a removal asked for while a stop is under way on a
    // running session wait for the stop, and are decided against what it
    // leaves: the session stopped. The first to have come starts it.
    let mut state = world_with(&["s1"]);
    command(&mut state, launch("s1"));
    command(&mut state, begin_exclusive("s1"));
    assert_eq!(command(&mut state, begin_start("s1")), Outcome::Held);
    command(&mut state, remove_session("s1"));
    assert_eq!(command(&mut state, begin_removal("s1")), Outcome::Held);
    report(&mut state, run_ending("s1"));
    step(&mut state, run_ended("s1"));
    let stopped = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        stopped.released,
        [(0, admitted(Operation::Exclusive("s1".into())))]
    );
    assert_eq!(state.held().len(), 1);
}

/// [`world`] with `s1` found in the backend's records, stored, in the
/// isolated workspace.
fn world_with_found() -> State {
    let mut existing = existing();
    existing.sessions.push(row("s1", W, true));
    State::restored(existing)
}

// verifies: FLOW-RESUME-SESSION-R14
#[test]
fn a_found_session_core_runs_holds_back_removing_its_workspace_and_project() {
    // Running, and while its run winds down: refused. Once Core has let go
    // of the run: admitted.
    let mut state = world_with_found();
    command(&mut state, launch("s1"));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        Outcome::Refused(Refusal::WorkspaceHasSessions)
    );
    report(&mut state, run_ending("s1"));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        Outcome::Refused(Refusal::WorkspaceHasSessions)
    );
    step(&mut state, run_ended("s1"));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        admitted(Operation::RemoveWorkspace(W.into()))
    );

    // A start under way when the removal is asked for: the removal waits
    // to see what it comes to. It launches: refused.
    let mut state = world_with_found();
    command(&mut state, begin_start("s1"));
    assert_eq!(command(&mut state, remove_workspace(W)), Outcome::Held);
    command(&mut state, launch("s1"));
    let started = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        started.released,
        [(0, Outcome::Refused(Refusal::WorkspaceHasSessions))]
    );
    // It ends without launching: admitted.
    let mut state = world_with_found();
    command(&mut state, begin_start("s1"));
    command(&mut state, remove_workspace(W));
    let failed = step(
        &mut state,
        Input::Report(Report::Ended(Operation::Exclusive("s1".into()))),
    );
    assert_eq!(
        failed.released,
        [(0, admitted(Operation::RemoveWorkspace(W.into())))]
    );

    // The removal first: no backend starts in the folder on its way out.
    let mut state = world_with_found();
    command(&mut state, remove_workspace(W));
    command(&mut state, begin_start("s1"));
    assert_eq!(
        command(&mut state, launch("s1")),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );

    // Any one of several found there holds it back.
    let mut two_found = existing();
    two_found.sessions.push(row("s1", W, true));
    two_found.sessions.push(row("s2", W, true));
    let mut state = State::restored(two_found);
    command(&mut state, launch("s2"));
    assert_eq!(
        command(&mut state, remove_workspace(W)),
        Outcome::Refused(Refusal::WorkspaceHasSessions)
    );

    // The same for a project and a session found in its own folder.
    let mut found_in_root = existing();
    found_in_root.workspaces.truncate(1);
    found_in_root.sessions.push(row("s1", ROOT, true));
    let mut state = State::restored(found_in_root.clone());
    command(&mut state, launch("s1"));
    assert_eq!(
        command(&mut state, remove_project()),
        Outcome::Refused(Refusal::ProjectHasSessions)
    );
    let mut state = State::restored(found_in_root);
    command(&mut state, remove_project());
    assert_eq!(
        command(&mut state, launch("s1")),
        Outcome::Refused(Refusal::WorkspaceHeld)
    );
}

// -- Every input in every state (ARCH-CORE-R18) -----------------------------

/// Every command and report over a world of one project in one folder, its
/// root and one isolated workspace, and `sessions`. Without `runs`, those
/// that begin or end a run, wait for one, or close Core are left out.
fn every_input(sessions: &[&str], runs: bool, sections: bool, sends: bool) -> Vec<Input> {
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
        if sections {
            inputs.push(Input::Command(archive(session)));
            inputs.push(Input::Command(arrange(session, None)));
            inputs.push(Input::Command(arrange(session, Some(SECTION))));
        }
        if sends {
            inputs.push(Input::Command(archive(session)));
            inputs.push(Input::Command(send(session)));
            inputs.push(Input::Command(send_to_running(session)));
            inputs.push(Input::Command(interrupt(session)));
            inputs.push(Input::Command(fork(session)));
            let ended = Report::Ended(Operation::Fork(session.to_string()));
            inputs.push(Input::Report(ended));
            for until in [SendIs::Taken, SendIs::Free, SendIs::Cancelled] {
                inputs.push(Input::Command(await_send(session, until)));
            }
            for stage in [
                SendStage::Checking,
                SendStage::Starting,
                SendStage::HandingOver,
            ] {
                let session = session.to_string();
                inputs.push(Input::Report(Report::SendStage { session, stage }));
            }
            for operation in [Operation::Send, Operation::Archive] {
                let ended = Report::Ended(operation(session.to_string()));
                inputs.push(Input::Report(ended));
            }
            // The writes a send's row reads.
            for change in [
                RowChange::Archived(true),
                RowChange::Archived(false),
                RowChange::Deleting(Deleting::Running),
            ] {
                let session = session.to_string();
                inputs.push(Input::Report(Report::RowWritten { session, change }));
            }
        }
        if runs {
            inputs.push(Input::Command(begin_start(session)));
            inputs.push(Input::Command(begin_removal(session)));
            inputs.push(Input::Command(launch(session)));
            inputs.push(Input::Command(await_run_end(session)));
        }
    }
    inputs.push(Input::Command(Command::CreateWorkspace {
        project: P.into(),
        workspace: W.into(),
    }));

    if sections {
        inputs.push(Input::Command(remove_section(SECTION)));
        let section = || SECTION.to_string();
        for report in [
            Report::SectionStored { section: section() },
            Report::SectionRemoved { section: section() },
            Report::Ended(Operation::RemoveSection(section())),
        ] {
            inputs.push(Input::Report(report));
        }
        for &session in sessions {
            for operation in [Operation::Archive, Operation::Arrange] {
                let ended = Report::Ended(operation(session.to_string()));
                inputs.push(Input::Report(ended));
            }
        }
    }
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
        // Its row, as any of the commands that admit it would store it.
        for workspace in workspaces {
            for found in [false, true] {
                reports.push(stored_in(workspace, &session, found));
            }
        }
        // Every kind of write, checked and not explored from; in a world
        // with sends, the writes a send's row reads are explored, above.
        if !sends {
            reports.extend(
                writes(&session)
                    .into_iter()
                    .map(|change| Report::RowWritten {
                        session: session.clone(),
                        change,
                    }),
            );
        }
        reports.extend([
            Report::SessionGone {
                session: session.clone(),
            },
            Report::Ended(Operation::CreateSession(session.clone())),
            Report::Ended(Operation::DeleteSession(session.clone())),
            Report::Ended(Operation::RemoveSession(session.clone())),
            Report::Ended(Operation::Exclusive(session.clone())),
        ]);
        if runs {
            reports.extend([
                Report::Ended(Operation::Run(session.clone())),
                run_ending(&session),
            ]);
        }
    }
    if runs {
        reports.push(Report::Closing);
    }
    // Whoever waits at any place of the queue may stop waiting.
    for place in 0..3 {
        reports.push(Report::GaveUp { place });
    }
    inputs.extend(reports.into_iter().map(Input::Report));
    // The send's world leaves the project and its workspaces as they are:
    // a send's row reads its session, its session's own delete, an archive
    // and the run, never a project's or a workspace's phase, and the other
    // worlds take those through every phase.
    if sends {
        inputs.retain(|input| !about_a_place(input));
    }
    inputs
}

/// Whether `input` is about a project or a workspace, not a session.
fn about_a_place(input: &Input) -> bool {
    let operation = |operation: &Operation| {
        matches!(
            operation,
            Operation::AddProject(_)
                | Operation::RemoveProject(_)
                | Operation::CreateWorkspace(_)
                | Operation::RemoveWorkspace(_)
        )
    };
    match input {
        Input::Command(command) => matches!(
            command,
            Command::AddProject { .. }
                | Command::RemoveProject { .. }
                | Command::CreateWorkspace { .. }
                | Command::RemoveWorkspace { .. }
        ),
        Input::Report(Report::Ended(ended)) => operation(ended),
        Input::Report(report) => matches!(
            report,
            Report::ProjectStored { .. }
                | Report::ProjectRemoved { .. }
                | Report::ProjectRemovalRefused { .. }
                | Report::WorkspaceReady { .. }
                | Report::WorkspaceCreateFailed { .. }
                | Report::WorkspaceRemoved { .. }
                | Report::WorkspaceRemovalRefused { .. }
                | Report::WorkspaceRemovalFailed { .. }
                | Report::WorkspaceErrored { .. }
                | Report::WorkspaceDeleted { .. }
        ),
    }
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
        // An arrangement under way into a section: the section is there,
        // and is not being removed.
        if let Some(Exclusive::Arrange { into: Some(into) }) = &session.exclusive {
            assert_eq!(
                state.section(into),
                Some(SectionPhase::Present),
                "{state:#?}"
            );
        }
        // A send is cancelled or handed over, never both.
        if let Some(send) = session.send {
            assert!(
                !(send.canceled && send.stage == SendStage::HandingOver),
                "{state:#?}"
            );
        }
        // A row is the row of the session it is kept for.
        if let Some(row) = &session.row {
            let named = (&row.id, &row.workspace_id, row.started_elsewhere);
            let kept = (id, &session.workspace, session.found);
            assert_eq!(named, kept, "{state:#?}");
        }
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
        // No backend runs in a folder on its way out: every session found
        // there is stopped.
        let runs = state
            .sessions()
            .any(|(_, s)| &s.workspace == id && s.run != Run::Stopped);
        assert!(
            !(removal && runs),
            "workspace {id} is held for removal with a session running: {state:#?}"
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
    for (id, phase) in state.sections() {
        match phase {
            SectionPhase::Removing => {
                reports.push(Report::Ended(Operation::RemoveSection(id.clone())));
            }
            SectionPhase::Present => {}
        }
    }
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
        if !session.stored() {
            reports.push(Report::Ended(Operation::CreateSession(id.clone())));
        }
        if session.removing {
            reports.push(Report::Ended(Operation::RemoveSession(id.clone())));
        }
        match &session.exclusive {
            Some(Exclusive::Archive) => {
                reports.push(Report::Ended(Operation::Archive(id.clone())));
            }
            Some(Exclusive::Arrange { .. }) => {
                reports.push(Report::Ended(Operation::Arrange(id.clone())));
            }
            Some(Exclusive::Other) => {
                reports.push(Report::Ended(Operation::Exclusive(id.clone())));
            }
            None => {}
        }
        if session.send.is_some() {
            reports.push(Report::Ended(Operation::Send(id.clone())));
        }
        if session.forking {
            reports.push(Report::Ended(Operation::Fork(id.clone())));
        }
        match session.run {
            Run::Running | Run::Ending => {
                reports.push(Report::Ended(Operation::Run(id.clone())));
            }
            Run::Stopped => {}
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
    // A reporter waiting for a send to take the slot waits on nothing that
    // is under way: it leaves when its reporter does (`Admission::ask`
    // withdraws it).
    let for_a_beginning = |held: &Command| {
        let until = SendIs::Taken;
        matches!(held, Command::AwaitSend { until: u, .. } if *u == until)
    };
    let mut settling = state.clone();
    for _ in 0..16 {
        if settling.held().iter().all(for_a_beginning) {
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
    match (input, outcome) {
        // An interrupt that is done marks the send cancelled, and that is
        // all it changes.
        (Input::Command(Command::Interrupt { session }), Outcome::Done) => {
            let was = before.session(session).and_then(|entry| entry.send);
            let was = was.expect("a send to cancel");
            assert_ne!(was.stage, SendStage::HandingOver, "{}", context());
            let mut marked = before.clone();
            step(&mut marked, input.clone());
            assert_eq!(&marked, after, "{}", context());
            let now = after.session(session).and_then(|entry| entry.send);
            let cancelled = Sending {
                canceled: true,
                ..was
            };
            assert_eq!(now, Some(cancelled), "{}", context());
            let unmarked = |state: &State| {
                let sessions = state.sessions().map(|(id, entry)| {
                    let mut entry = entry.clone();
                    entry.send = None;
                    (id.clone(), entry)
                });
                sessions.collect::<Vec<_>>()
            };
            assert_eq!(unmarked(before), unmarked(after), "{}", context());
            // And whoever waited to hear of it is told.
            let told = await_send(session, SendIs::Cancelled);
            let still: Vec<_> = before.held().iter().filter(|held| **held != told).collect();
            assert_eq!(
                still,
                after.held().iter().collect::<Vec<_>>(),
                "{}",
                context()
            );
        }
        (_, Outcome::Refused(_) | Outcome::Done | Outcome::Stale) => {
            assert_eq!(before, after, "{}", context());
        }
        (_, Outcome::Admitted(_) | Outcome::Held | Outcome::Noted) => {}
    }
    // A send is given the slot only when it is free, and its session is
    // neither being archived, archived, nor being deleted.
    if let (Input::Command(Command::Send { session, resumes }), Outcome::Admitted(_)) =
        (input, outcome)
    {
        let was = before.session(session).expect("an admitted session");
        let row = was.row.as_ref().expect("a stored row");
        assert_eq!(was.send, None, "{}", context());
        assert!(!was.forking, "{}", context());
        assert!(!before.being_archived(session), "{}", context());
        assert!(!row.archived && row.deleting.is_none(), "{}", context());
        let own_delete = before
            .workspace(&was.workspace)
            .and_then(|w| w.deleting.clone());
        assert_ne!(own_delete.as_ref(), Some(session), "{}", context());
        assert!(*resumes || was.run != Run::Stopped, "{}", context());
    }
    // No session is deleted while a fork of it is starting, and a fork
    // begins only when no message is on its way to its original, no other
    // fork of it is starting, and its workspace is not held.
    if let (Input::Command(Command::DeleteSession { session }), Outcome::Admitted(_)) =
        (input, outcome)
    {
        let was = before.session(session).expect("an admitted session");
        assert!(!was.forking, "{}", context());
    }
    if let (Input::Command(Command::Fork { session }), Outcome::Admitted(_)) = (input, outcome) {
        let was = before.session(session).expect("an admitted session");
        assert!(was.send.is_none() && !was.forking, "{}", context());
        assert!(!before.workspace_held(&was.workspace), "{}", context());
        assert!(
            was.row.as_ref().is_some_and(|row| row.deleting.is_none()),
            "{}",
            context()
        );
    }
    // A send is handed over only when it was not cancelled.
    if let (Input::Report(Report::SendStage { session, stage }), Outcome::Noted) = (input, outcome)
        && *stage == SendStage::HandingOver
    {
        let was = before.session(session).and_then(|entry| entry.send);
        assert!(was.is_some_and(|send| !send.canceled), "{}", context());
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
    // An archive or an arrangement is the session's one exclusive
    // operation: admitted only when none was under way.
    if let (
        Input::Command(Command::Archive { session } | Command::Arrange { session, .. }),
        Outcome::Admitted(_),
    ) = (input, outcome)
    {
        let was = before.session(session).expect("an admitted session");
        assert_eq!(was.exclusive, None, "{}", context());
    }
    // A section is removed only by a removal of it that is under way.
    for (id, _) in before.sections() {
        let removing = before.section(id) == Some(SectionPhase::Removing);
        assert!(after.section(id).is_some() || removing, "{}", context());
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
    // A run begins only on a stopped session, and never once Core is
    // closing; nor is a session created then.
    for (id, session) in after.sessions() {
        let was = before.session(id).map(|s| s.run);
        let began = session.run == Run::Running && was != Some(Run::Running);
        if began {
            assert_eq!(was, Some(Run::Stopped), "{}", context());
            assert!(!before.closing(), "{}", context());
        }
        assert!(
            was.is_some() || !before.closing(),
            "a session was created while closing: {}",
            context()
        );
    }
    // A session has one exclusive operation at most: one is admitted only
    // on a session with none.
    if let (
        Input::Command(
            Command::BeginExclusive { session }
            | Command::BeginStart { session }
            | Command::BeginRemoval { session },
        ),
        Outcome::Admitted(_),
    ) = (input, outcome)
    {
        assert!(!exclusive(before, session), "{}", context());
    }
}

/// Applies the inputs of [`every_input`] over `sessions` (and the one
/// project and its two workspaces) to every state reachable from nothing
/// and from [`world`], checking each state and each step, and returns the
/// states. States with more than `held` commands held are not explored
/// further.
/// Whether `before` and `after` differ in nothing but the rows of the
/// sessions `changed` names.
fn same_but_rows(before: &State, after: &State, changed: impl Fn(&str) -> bool) -> bool {
    let masked = |state: &State| {
        let sessions = state.sessions().map(|(id, entry)| {
            let mut entry = entry.clone();
            if changed(id) {
                entry.row = entry
                    .row
                    .map(|row| self::row(&row.id, &row.workspace_id, false));
            }
            (id.clone(), entry)
        });
        sessions.collect::<Vec<_>>()
    };
    masked(before) == masked(after)
        && before.projects().eq(after.projects())
        && before.workspaces().eq(after.workspaces())
        && before.held() == after.held()
        && before.closing() == after.closing()
}

fn explore(
    sessions: &[&str],
    held: usize,
    runs: bool,
    sections: bool,
    sends: bool,
) -> HashSet<State> {
    let inputs = every_input(sessions, runs, sections, sends);
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
            match (input, &stepped.outcome) {
                // An interrupt that is done marked a send cancelled.
                (Input::Command(Command::Interrupt { .. }), Outcome::Done) => {}
                (_, Outcome::Refused(_) | Outcome::Done | Outcome::Held | Outcome::Stale) => {
                    assert!(stepped.released.is_empty(), "{input:?}");
                }
                (_, Outcome::Admitted(_) | Outcome::Noted) => {}
            }
            // A write changes one session's row and nothing else, answers no
            // held command, and no row of the table reads a row's contents:
            // the state it leads to decides every input as this one does, so
            // it is checked here and not explored again.
            match input {
                Input::Report(Report::RowWritten { session, .. }) if !sends => {
                    assert!(stepped.released.is_empty(), "{input:?}");
                    assert!(
                        same_but_rows(&before, &after, |id| id == session),
                        "{input:?}"
                    );
                    check_state(&after);
                    continue;
                }
                Input::Command(_) | Input::Report(_) => {}
            }
            if after.held().len() <= held && seen.insert(after.clone()) {
                queue.push_back(after);
            }
        }
    }
    assert!(steps > seen.len());
    seen
}

// verifies: ARCH-CORE-R18
// verifies: ARCH-CORE-R15
// verifies: ARCH-CORE-R17
#[test]
fn every_input_in_every_reachable_state_keeps_what_admission_promises() {
    // The small world: one project, its two workspaces and one session,
    // given every input there is, with up to two commands held at once.
    // Every phase of a project, a workspace, a session and its run, every
    // operation, shutdown, and every way one command waits behind another
    // is reached.
    let whole = explore(&["s1"], 2, true, false, false);
    assert!(whole.len() > 10_000, "only {} states", whole.len());
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
            whole.iter().any(|s| phase(s, W) == Some(wanted)),
            "{wanted:?} was never reached"
        );
    }
    for wanted in [Run::Stopped, Run::Running, Run::Ending] {
        assert!(
            whole.iter().any(|s| run_of(s, "s1") == Some(wanted)),
            "{wanted:?} was never reached"
        );
    }
    assert!(whole.iter().any(|s| s.closing()));
    assert!(whole.iter().any(|s| s.held().len() == 2));
    assert!(
        whole
            .iter()
            .any(|s| s.workspace(W).is_some_and(|w| w.deleting.is_some()))
    );
    assert!(whole.iter().any(|s| exclusive(s, "s1")));

    // Two sessions with every input is over a million states, minutes of
    // run. What two sessions add is what one does to the other through
    // the workspace they share: a delete's hold, a row still being stored,
    // a removal that waits or is refused. Those are explored with two
    // sessions and one command held at a time, without the inputs of a
    // run, which act on one session alone (a removal's "does any found
    // session run" over several is a fed sequence).
    let pairs = explore(&["s1", "s2"], 1, false, false, false);
    assert!(pairs.len() > 1_000, "only {} states", pairs.len());
    assert!(
        pairs
            .iter()
            .any(|s| s.sessions().filter(|(_, session)| session.stored()).count() == 2)
    );

    // Archive, arrange, a section's coming and going, and a send's
    // question, with every other input but a run's: their rows read the
    // session's exclusive operation, whether its row is stored, and the
    // section's phase, never the run. One session, one section, up to two
    // commands held at once.
    let arranged = explore(&["s1"], 2, false, true, false);
    assert!(arranged.len() > 1_000, "only {} states", arranged.len());
    for wanted in [SectionPhase::Present, SectionPhase::Removing] {
        assert!(
            arranged.iter().any(|s| s.section(SECTION) == Some(wanted)),
            "{wanted:?} was never reached"
        );
    }
    let has = |wanted: fn(&Exclusive) -> bool| {
        let under_way = |s: &State| s.session("s1").and_then(|entry| entry.exclusive.clone());
        arranged
            .iter()
            .any(|s| under_way(s).as_ref().is_some_and(wanted))
    };
    assert!(has(|e| matches!(e, Exclusive::Archive)));
    assert!(has(|e| matches!(e, Exclusive::Arrange { into: Some(_) })));
    assert!(has(|e| matches!(e, Exclusive::Arrange { into: None })));
    assert!(has(|e| matches!(e, Exclusive::Other)));
    // An archive waiting its turn, and a removal waiting for an arrangement.
    let held = |wanted: fn(&Command) -> bool| arranged.iter().any(|s| s.held().iter().any(wanted));
    assert!(held(|c| matches!(c, Command::Archive { .. })));
    assert!(held(|c| matches!(c, Command::RemoveSection { .. })));
    assert!(held(|c| matches!(c, Command::Arrange { .. })));

    // The send slot: one session, with every input of a run (a stopped
    // session takes a message only from a sender that can start it), the
    // archive, the session's delete and the row's marks a send is refused
    // for, and one command held at a time.
    let sent = explore(&["s1"], 1, true, false, true);
    assert!(sent.len() > 1_000, "only {} states", sent.len());
    let reached = |wanted: Sending| sent.iter().any(|s| sending(s, "s1") == Some(wanted));
    for stage in [
        SendStage::Checking,
        SendStage::Starting,
        SendStage::HandingOver,
    ] {
        let canceled = false;
        assert!(reached(Sending { stage, canceled }), "{stage:?}");
    }
    for stage in [SendStage::Checking, SendStage::Starting] {
        let canceled = true;
        assert!(reached(Sending { stage, canceled }), "{stage:?} cancelled");
    }
    // A send under way in a session that has since been archived, marked
    // deleting, or lost its run: each was let in before.
    let with_send =
        |wanted: fn(&State) -> bool| sent.iter().any(|s| sending(s, "s1").is_some() && wanted(s));
    assert!(with_send(|s| s.row("s1").is_some_and(|row| row.archived)));
    assert!(with_send(|s| s
        .row("s1")
        .is_some_and(|row| row.deleting.is_some())));
    assert!(with_send(|s| run_of(s, "s1") == Some(Run::Ending)));
    // A fork starting, on its own, beside the original's exclusive
    // operation, and in a session that has since been marked deleting.
    let with_fork = |wanted: fn(&State) -> bool| sent.iter().any(|s| forking(s, "s1") && wanted(s));
    assert!(with_fork(|s| !exclusive(s, "s1")));
    assert!(with_fork(|s| exclusive(s, "s1")));
    assert!(with_fork(|s| s
        .row("s1")
        .is_some_and(|row| row.deleting.is_some())));
    assert!(
        !sent
            .iter()
            .any(|s| forking(s, "s1") && sending(s, "s1").is_some())
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
    session.end(stored_in(ROOT, "s2", false));
    assert!(
        admission
            .snapshot()
            .session("s2")
            .is_some_and(|s| s.stored())
    );
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
    stored.sessions.push(row("s1", W, false));
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

// verifies: ARCH-CORE-R17
// Paused time: each limit passes when the test's clock says so.
#[tokio::test(start_paused = true)]
async fn each_waiter_gives_up_at_its_own_limit_and_withdraws_only_its_own_command() {
    use std::time::Duration;
    let admission = Arc::new(Admission::default());
    let mut stored = existing();
    stored.sessions.push(row("s1", W, false));
    admission.restore(stored);
    // A run that was announced ended and that Core never lets go of.
    let _run = admission.launch("s1").expect("launched");
    admission.report(run_ending("s1"));

    let began = tokio::time::Instant::now();
    let start = |limit: u64| {
        let admission = Arc::clone(&admission);
        async move {
            let answer = admission
                .begin_start("s1", Duration::from_secs(limit))
                .await
                .map(|_| ());
            (answer, began.elapsed().as_secs())
        }
    };
    // Two resumes: the same command, held twice. The one that came first
    // waits longer, so the one behind it in the queue gives up first.
    let patient = tokio::spawn(start(60));
    tokio::time::sleep(Duration::from_secs(10)).await;
    let second = tokio::spawn(start(30));
    // And a send asking whether the session runs, five seconds later.
    tokio::time::sleep(Duration::from_secs(5)).await;
    let asking = tokio::spawn({
        let admission = Arc::clone(&admission);
        async move {
            let answer = admission.run_settled("s1", Duration::from_secs(30)).await;
            (answer, began.elapsed().as_secs())
        }
    });

    // Never left waiting past every limit: a waiter that withdrew another's
    // command would wait for ever on its own.
    let all = async {
        (
            second.await.expect("joined"),
            asking.await.expect("joined"),
            patient.await.expect("joined"),
        )
    };
    let (second, asking, patient) = tokio::time::timeout(Duration::from_secs(600), all)
        .await
        .expect("every waiter was answered");
    assert_eq!(second, (Err(Refusal::GaveUp), 40));
    assert_eq!(asking, (Err(Refusal::GaveUp), 45));
    // The first to come was not answered by the others' limits.
    assert_eq!(patient, (Err(Refusal::GaveUp), 60));
    assert!(admission.snapshot().held().is_empty());
}

// verifies: ARCH-CORE-R16
#[tokio::test]
async fn an_operation_with_several_holders_ends_when_the_last_lets_go() {
    let admission = Arc::new(Admission::default());
    let mut stored = existing();
    stored.sessions.push(row("s1", W, false));
    admission.restore(stored);
    let marked = |admission: &Admission| {
        let state = admission.snapshot();
        state.session("s1").is_some_and(|entry| entry.forking)
    };

    // A fork's reads, its registry check and its start share the
    // operation.
    let reads = Arc::new(admit(&admission, fork("s1")).await);
    let registry_check = Arc::clone(&reads);
    let start = Arc::clone(&reads);
    assert!(marked(&admission));
    // The reads end, in a task of their own: the check and the start still
    // hold it.
    tokio::spawn(async move { drop(reads) }).await.unwrap();
    assert!(marked(&admission));
    drop(registry_check);
    assert!(marked(&admission));
    assert!(matches!(
        admission.begin_send("s1", true),
        Err(Refusal::BeingForked)
    ));
    // The last lets go: the mark goes with it.
    drop(start);
    assert!(!marked(&admission));
    assert!(admission.begin_send("s1", true).is_ok());
}

// verifies: ARCH-CORE-R17
#[tokio::test]
async fn a_question_whose_asker_left_is_withdrawn_and_one_that_stays_is_answered() {
    let admission = Arc::new(Admission::default());
    let mut stored = existing();
    stored.sessions.push(row("s1", W, false));
    admission.restore(stored);
    let ask = |until: SendIs| {
        let admission = Arc::clone(&admission);
        tokio::spawn(async move { admission.ask(await_send("s1", until)).await })
    };

    // A reporter waiting for a send that never comes, and then gone: its
    // question does not stay behind.
    let waiting = ask(SendIs::Taken);
    until_held(&admission).await;
    waiting.abort();
    let _ = waiting.await;
    assert!(admission.snapshot().held().is_empty());

    // One that stays is answered when a send takes the slot, and a
    // question that is so already is answered at once.
    let waiting = ask(SendIs::Taken);
    until_held(&admission).await;
    let sending = admission.begin_send("s1", true).expect("the slot is free");
    let waited = tokio::time::timeout(std::time::Duration::from_secs(5), waiting);
    assert_eq!(
        waited.await.expect("answered in time").expect("joined"),
        Outcome::Done
    );
    assert!(admission.snapshot().held().is_empty());
    assert_eq!(
        admission.ask(await_send("s1", SendIs::Taken)).await,
        Outcome::Done
    );

    // A send hears it was cancelled; one that ends first hears there is
    // none.
    let cancelled = ask(SendIs::Cancelled);
    until_held(&admission).await;
    assert!(admission.interrupt_send("s1"));
    let waited = tokio::time::timeout(std::time::Duration::from_secs(5), cancelled);
    assert_eq!(
        waited.await.expect("answered in time").expect("joined"),
        Outcome::Done
    );
    drop(sending);
    assert_eq!(
        admission.ask(await_send("s1", SendIs::Cancelled)).await,
        Outcome::Refused(Refusal::NoSendToCancel)
    );
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
