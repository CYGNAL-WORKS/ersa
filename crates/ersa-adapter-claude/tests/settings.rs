//! What the adapter reports of a session's model, effort and permission
//! mode: a live session's from its stream (PROTO-CLAUDE-CODE-STREAM-R40),
//! any session's from its record (R47).

mod common;

use common::{fixture, log};
use ersa_adapter::Direction::{Backend, Host};
use ersa_adapter::{Projector, RawLine};
use ersa_adapter_claude::ClaudeProjector;
use serde_json::Value;

/// The live report after each line that changed it: the line's kind, then
/// the model and permission mode the projector reports.
fn live_changes(lines: &[RawLine]) -> Vec<(String, Option<String>, Option<String>)> {
    let mut projector = ClaudeProjector::new();
    let mut changes = Vec::new();
    let mut last = (None, None);
    for line in lines {
        projector.project(line);
        let now = (
            projector.reported_model().map(String::from),
            projector.reported_permission_mode().map(String::from),
        );
        if now != last {
            changes.push((kind(line), now.0.clone(), now.1.clone()));
            last = now;
        }
    }
    changes
}

/// `type/subtype` of a raw line, or `type` alone.
fn kind(line: &RawLine) -> String {
    let value: Value = serde_json::from_str(&line.line).unwrap();
    match (value["type"].as_str(), value["subtype"].as_str()) {
        (Some(t), Some(s)) => format!("{t}/{s}"),
        (Some(t), None) => t.to_string(),
        _ => String::new(),
    }
}

fn change(kind: &str, model: Option<&str>, mode: &str) -> (String, Option<String>, Option<String>) {
    (
        kind.to_string(),
        model.map(String::from),
        Some(mode.to_string()),
    )
}

const OPUS: &str = "claude-opus-5-5";
const HAIKU: &str = "claude-haiku-5-5";

/// Between turns (fixture 25): the mode from `initialize` before the first
/// turn, then the first `system/init`; `set_model`'s answer and the
/// `system/status` after `set_permission_mode` change them at once, before
/// the next turn's `system/init` repeats them.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn a_live_session_reports_each_new_model_and_mode_as_the_stream_gives_it() {
    let lines = fixture("2.1.316/25-controls-between-turns").raw_log();
    assert_eq!(
        live_changes(&lines),
        [
            change("control_response", None, "default"),
            change("system/init", Some(OPUS), "default"),
            change("control_response", Some(HAIKU), "default"),
            change("system/status", Some(HAIKU), "acceptEdits"),
        ]
    );
}

/// During a turn (fixture 26) the same answers change the report at once,
/// long before the next `system/init`.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn controls_sent_during_a_turn_are_reported_from_their_answers() {
    let lines = fixture("2.1.316/26-controls-during-turn").raw_log();
    let changes = live_changes(&lines);
    assert_eq!(
        changes,
        [
            change("control_response", None, "default"),
            change("system/init", Some(OPUS), "default"),
            change("control_response", Some(HAIKU), "default"),
            change("system/status", Some(HAIKU), "acceptEdits"),
        ]
    );
}

/// A later `system/init` that reports something else wins over an earlier
/// answer: the stream's newest report is the one reported.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn each_system_init_reports_the_model_and_mode_again() {
    let lines = log(&[
        (
            Backend,
            r#"{"type":"system","subtype":"init","model":"claude-opus-5-5","permissionMode":"plan","session_id":"s"}"#,
        ),
        (
            Backend,
            r#"{"type":"result","subtype":"success","is_error":false,"session_id":"s"}"#,
        ),
        (
            Backend,
            r#"{"type":"system","subtype":"init","model":"claude-sonnet-5-5","permissionMode":"auto","session_id":"s"}"#,
        ),
    ]);
    assert_eq!(
        live_changes(&lines),
        [
            change("system/init", Some(OPUS), "plan"),
            change("system/init", Some("claude-sonnet-5-5"), "auto"),
        ]
    );
}

/// Only the answer to the host's own `set_model` names the model: another
/// request's answer that happens to carry `model` does not, and a status
/// line without `permissionMode` leaves the mode.
// verifies: PROTO-CLAUDE-CODE-STREAM-R40
#[test]
fn only_set_models_answer_and_status_lines_with_a_mode_change_the_report() {
    let lines = log(&[
        (
            Backend,
            r#"{"type":"system","subtype":"init","model":"claude-opus-5-5","permissionMode":"default","session_id":"s"}"#,
        ),
        (
            Host,
            r#"{"type":"control_request","request_id":"other-1","request":{"subtype":"get_context_usage"}}"#,
        ),
        (
            Backend,
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"other-1","response":{"model":"claude-haiku-5-5"}}}"#,
        ),
        (
            Backend,
            r#"{"type":"system","subtype":"status","status":"requesting","session_id":"s"}"#,
        ),
        (
            Host,
            r#"{"type":"control_request","request_id":"m-2","request":{"subtype":"set_model","model":"sonnet"}}"#,
        ),
        (
            Backend,
            r#"{"type":"control_response","response":{"subtype":"success","request_id":"m-2","response":{"model":"claude-sonnet-5-5"}}}"#,
        ),
    ]);
    assert_eq!(
        live_changes(&lines),
        [
            change("system/init", Some(OPUS), "default"),
            change("control_response", Some("claude-sonnet-5-5"), "default"),
        ]
    );
}
