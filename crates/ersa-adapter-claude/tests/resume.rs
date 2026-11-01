//! Resuming a session: the driver continues the session whose `system/init`
//! id it is given, and checks the CLI's live-session registry first
//! (PROTO-CLAUDE-CODE-STREAM, Invocation and Live-session registry).

mod common;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterError, SessionConfig};
use ersa_adapter_claude::record::{live_session, record_path};
use ersa_adapter_claude::{ClaudeAdapter, invocation_args};
use serde_json::{Value, json};

const SESSION_ID: &str = "d34f0c1b-64ee-437e-a4e3-3487df29a506";

fn config(extra: Value) -> SessionConfig {
    SessionConfig {
        cwd: "C:/work".into(),
        permission_mode: "default".into(),
        model: None,
        extra,
    }
}

/// A stub CLI that answers `initialize` and then waits for input.
fn ready_stub() -> FakeTransport {
    let init_answer = fixture("11-stream-permission-prompt-tool-stdio").stdout[0].clone();
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|line: &str| {
            let value: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
            match value["request"]["subtype"].as_str() {
                Some("initialize") => Ok(()),
                _ => Err(format!("expected initialize, got {line}")),
            }
        })
        .then_stdout([init_answer])
        .hold_open()
}

fn adapter() -> ClaudeAdapter {
    ClaudeAdapter::new()
        .with_handshake_timeout(Duration::from_secs(5))
        .with_stop_grace(Duration::from_millis(200))
}

/// The value that follows `flag` in `args`, if the flag is there.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).map(String::as_str)
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R14
#[tokio::test]
async fn resuming_passes_the_init_session_id_to_resume() {
    // The id Ersa stored is the one the session's `system/init` reported.
    let init = fixture("2.1.316/00-session-tools")
        .stdout
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|m| m["type"] == "system" && m["subtype"] == "init")
        .expect("the fixture has a system/init");
    let id = init["session_id"].as_str().unwrap().to_string();

    let transport = ready_stub();
    let session = adapter()
        .start(
            Arc::new(transport.clone()),
            config(json!({ "resume": id })),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");
    let args = transport.spawned()[0].args.clone();
    assert_eq!(flag_value(&args, "--resume"), Some(id.as_str()), "{args:?}");
    session.stop().await.expect("stop");

    // And the CLI kept that id when it resumed: the resume fixture's init
    // reports the same session.
    let resumed = fixture("2.1.316/01-session-resume")
        .stdout
        .iter()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|m| m["type"] == "system" && m["subtype"] == "init")
        .expect("the fixture has a system/init");
    assert_eq!(resumed["session_id"], id.as_str());
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R14
#[test]
fn a_new_session_passes_no_resume() {
    assert_eq!(
        flag_value(&invocation_args(&config(Value::Null)), "--resume"),
        None
    );
    assert!(!invocation_args(&config(json!({}))).contains(&"--resume".to_string()));
}

#[tokio::test]
async fn a_resume_id_that_is_not_a_session_id_fails_start_without_spawning() {
    // Session ids can come from file names in the CLI's record directory; one
    // that looks like a flag must never reach the command line.
    for bad in [
        "--dangerously-skip-permissions",
        "",
        "abc",
        "../x",
        7.to_string().as_str(),
    ] {
        let transport = ready_stub();
        let result = adapter()
            .start(
                Arc::new(transport.clone()),
                config(json!({ "resume": bad })),
                Arc::new(RecordingSink::new()),
            )
            .await;
        assert!(
            matches!(result, Err(AdapterError::Protocol(_))),
            "{bad:?} was accepted"
        );
        assert!(transport.spawned().is_empty(), "{bad:?} spawned the CLI");
        assert_eq!(
            flag_value(
                &invocation_args(&config(json!({ "resume": bad }))),
                "--resume"
            ),
            None
        );
    }
    // A resume value that is not a string at all is refused too.
    let transport = ready_stub();
    let result = adapter()
        .start(
            Arc::new(transport.clone()),
            config(json!({ "resume": 42 })),
            Arc::new(RecordingSink::new()),
        )
        .await;
    assert!(matches!(result, Err(AdapterError::Protocol(_))));
    assert!(transport.spawned().is_empty());
}

/// A scratch `~/.claude` holding a registry file for `pid`.
fn registry_with(pid: u32, session_id: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(
        sessions.join(format!("{pid}.json")),
        json!({
            "pid": pid,
            "sessionId": session_id,
            "cwd": "C:/work",
            "startedAt": 1_793_000_000_000u64,
            "kind": "interactive",
            "entrypoint": "cli",
            "status": "idle",
        })
        .to_string(),
    )
    .unwrap();
    home
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R17
#[test]
fn a_running_process_with_the_session_open_is_reported() {
    // This test process stands in for a running CLI.
    let pid = std::process::id();
    let home = registry_with(pid, SESSION_ID);
    let live = live_session(home.path(), SESSION_ID).expect("reported");
    assert_eq!(live.pid, pid);
    assert_eq!(live.entrypoint.as_deref(), Some("cli"));
    // Another session is not.
    assert_eq!(
        live_session(home.path(), "0989e382-5a8f-4fdf-9738-bf02b1f049e1"),
        None
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R17
#[test]
fn a_registry_file_left_by_a_process_that_has_exited_is_not_reported() {
    // A process that has certainly exited: started and waited for here.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();
    let home = registry_with(pid, SESSION_ID);
    assert_eq!(live_session(home.path(), SESSION_ID), None);
}

#[test]
fn no_registry_or_a_broken_file_reports_nothing() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(live_session(home.path(), SESSION_ID), None);
    let sessions = home.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join("123.json"), "not json").unwrap();
    fs::write(sessions.join("124.json"), r#"{"pid":"x","sessionId":7}"#).unwrap();
    assert_eq!(live_session(home.path(), SESSION_ID), None);
}

#[test]
fn the_record_lives_under_the_encoded_working_directory() {
    let home = std::path::Path::new("/h/.claude");
    assert_eq!(
        record_path(home, r"D:\Projects\ersa", SESSION_ID),
        home.join("projects")
            .join("D--Projects-ersa")
            .join(format!("{SESSION_ID}.jsonl"))
    );
}
