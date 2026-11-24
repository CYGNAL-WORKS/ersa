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
