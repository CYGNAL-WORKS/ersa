//! How long the Shell waits for Core's answer to each method.

use std::time::Duration;

use ersa_shell::bridge::timeout_for;

/// A delete's folder alone can take longer than the 30 seconds most
/// requests get, so the Shell waits 10 minutes for its answer.
// verifies: PROTO-CORE-API-R115
#[test]
fn a_delete_is_waited_on_for_10_minutes() {
    assert_eq!(timeout_for("session.delete"), Duration::from_secs(10 * 60));
}

/// A fork can make a workspace, and that waits its turn at the project's
/// worktrees, so the Shell waits 10 minutes for its answer.
// verifies: PROTO-CORE-API-R120
#[test]
fn a_fork_is_waited_on_for_10_minutes() {
    assert_eq!(timeout_for("session.fork"), Duration::from_secs(10 * 60));
}

/// A resume may first put the session's folder back, which waits for a
/// put-away under way and for a turn at the project's worktrees, so the
/// Shell waits 10 minutes for its answer.
// verifies: FLOW-CLEAN-UP-R05
#[test]
fn a_resume_is_waited_on_for_10_minutes() {
    assert_eq!(timeout_for("session.resume"), Duration::from_secs(10 * 60));
}

/// An inspect measures a folder for up to 30 seconds and counts its changes
/// for up to 60, so the Shell waits 2 minutes for its answer.
// verifies: PROTO-CORE-API-R125
#[test]
fn a_clean_up_inspect_is_waited_on_for_2_minutes() {
    assert_eq!(timeout_for("cleanup.inspect"), Duration::from_secs(2 * 60));
}

/// A removal waits its turn at the project's worktrees and then removes a
/// folder, so the Shell waits 10 minutes for its answer.
// verifies: PROTO-CORE-API-R125
#[test]
fn a_clean_up_removal_is_waited_on_for_10_minutes() {
    assert_eq!(timeout_for("cleanup.remove"), Duration::from_secs(10 * 60));
}
