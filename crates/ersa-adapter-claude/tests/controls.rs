//! Changing a live session's model, effort and permission mode
//! (PROTO-CLAUDE-CODE-STREAM, Control requests): each change is the control
//! request the recordings show the host sending, and the adapter reads the
//! CLI's answer to it. The stub CLI answers as the CLI did in 2.1.316
//! fixture 26, where the three were sent during a turn.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterError, SessionConfig, SessionControl};
use ersa_adapter_claude::ClaudeAdapter;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(5);

/// The recording the controls were sent in, during a running turn.
const DURING_A_TURN: &str = "2.1.316/26-controls-during-turn";

fn adapter() -> ClaudeAdapter {
    ClaudeAdapter::new()
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
}

fn config() -> SessionConfig {
    SessionConfig {
        cwd: "C:/work".into(),
        permission_mode: "default".into(),
        model: None,
        effort: None,
        project_trusted: false,
        project_path: String::new(),
        backend_session_id: None,
        fork_of: None,
        resume: None,
        extra: Value::Null,
    }
}

/// The line the recording sent for control `subtype`, and the CLI's answer
/// to it.
fn recorded(subtype: &str) -> (Value, Value) {
    let recording = fixture(DURING_A_TURN);
    let sent: Value = recording
        .sent()
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["request"]["subtype"] == subtype)
        .expect("the recording sent the control");
    let id = sent["request_id"].as_str().unwrap();
    let answer: Value = recording
        .stdout
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["response"]["request_id"] == id)
        .expect("the CLI answered the control");
    (sent, answer)
}

/// `line` with its request id replaced by `id`: the stub's answers are fixed
/// in advance, so they carry the id the driver chooses for its first request
/// after `initialize`.
fn with_id(mut line: Value, id: &str) -> Value {
    if line.get("request_id").is_some() {
        line["request_id"] = json!(id);
    } else {
        line["response"]["request_id"] = json!(id);
    }
    line
}

/// A stub CLI that answers `initialize`, then answers the next stdin line
/// with `answer`.
fn answering(answer: Value) -> FakeTransport {
    let init_answer = fixture("11-stream-permission-prompt-tool-stdio").stdout[0].clone();
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([init_answer])
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([answer.to_string()])
        .hold_open()
}

/// Each control, with the request it is sent as and the id the driver gives
/// that request.
fn controls() -> [(SessionControl, &'static str, &'static str); 3] {
    [
        (
            SessionControl::Model("haiku".into()),
            "set_model",
            "ersa-set_model-2",
        ),
        (
            SessionControl::PermissionMode("acceptEdits".into()),
            "set_permission_mode",
            "ersa-set_permission_mode-2",
        ),
        (
            SessionControl::Effort("low".into()),
            "apply_flag_settings",
            "ersa-apply_flag_settings-2",
        ),
    ]
}

/// Each change is written as the recording's request, and returns once the
/// CLI answers success: `set_model` with the resolved model,
/// `set_permission_mode` with the mode, `apply_flag_settings` with no
/// `response` at all.
// verifies: PROTO-CLAUDE-CODE-STREAM-R41
#[tokio::test]
async fn each_change_is_the_recorded_control_request_and_its_answer_is_read() {
    for (control, subtype, id) in controls() {
        let (sent, answer) = recorded(subtype);
        let transport = answering(with_id(answer, id));
        let session = adapter()
            .start(
                Arc::new(transport.clone()),
                config(),
                Arc::new(RecordingSink::new()),
            )
            .await
            .expect("start");
        tokio::time::timeout(WAIT, session.change(control.clone()))
            .await
            .expect("answered in time")
            .unwrap_or_else(|e| panic!("{subtype}: {e}"));

        let written: Value = serde_json::from_str(&transport.stdin_lines()[1]).unwrap();
        assert_eq!(written, with_id(sent, id), "{subtype}");
        assert!(transport.stub_failures().is_empty());
        session.stop().await.unwrap();
    }
}

/// A change the CLI refuses is an error carrying the CLI's reason.
// verifies: PROTO-CLAUDE-CODE-STREAM-R41
#[tokio::test]
async fn a_refused_change_is_an_error_with_the_clis_reason() {
    for (control, subtype, id) in controls() {
        let transport = answering(json!({"type": "control_response", "response": {
            "subtype": "error", "request_id": id, "error": "not available"}}));
        let session = adapter()
            .start(
                Arc::new(transport),
                config(),
                Arc::new(RecordingSink::new()),
            )
            .await
            .expect("start");
        let error = tokio::time::timeout(WAIT, session.change(control))
            .await
            .expect("answered in time")
            .unwrap_err();
        assert!(
            matches!(&error, AdapterError::Protocol(m) if m.contains("not available")),
            "{subtype}: {error}"
        );
        session.stop().await.unwrap();
    }
}
