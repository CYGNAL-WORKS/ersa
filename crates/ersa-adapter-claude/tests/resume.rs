//! Resuming a session: the driver continues the session whose `system/init`
//! id it is given, and `live_session` reads the CLI's live-session registry,
//! which Core consults before it resumes one
//! (PROTO-CLAUDE-CODE-STREAM, Invocation and Live-session registry).

mod common;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterError, OpenElsewhere, SessionConfig};
use ersa_adapter_claude::record::{LiveCheck, live_session, record_path};
use ersa_adapter_claude::{ClaudeAdapter, invocation_args, resume_args};
use serde_json::{Value, json};

const SESSION_ID: &str = "d34f0c1b-64ee-437e-a4e3-3487df29a506";

fn config(extra: Value) -> SessionConfig {
    SessionConfig {
        cwd: "C:/work".into(),
        permission_mode: "default".into(),
        model: None,
        effort: None,
        project_trusted: false,
        project_path: String::new(),
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
    for extra in [Value::Null, json!({}), json!({ "resume": null })] {
        assert!(
            resume_args(&config(extra.clone())).unwrap().is_empty(),
            "{extra}"
        );
        assert_eq!(
            flag_value(&invocation_args(&config(extra)), "--resume"),
            None
        );
    }
}

#[tokio::test]
async fn a_null_resume_starts_a_new_session() {
    let transport = ready_stub();
    let session = adapter()
        .start(
            Arc::new(transport.clone()),
            config(json!({ "resume": null })),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");
    assert_eq!(flag_value(&transport.spawned()[0].args, "--resume"), None);
    session.stop().await.expect("stop");
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
        assert!(resume_args(&config(json!({ "resume": bad }))).is_err());
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

/// A pid no process can have: far above Linux's highest (2^22), and Windows,
/// which ignores a pid's low two bits, reads it as 0x3FFF_FFFC, far beyond any
/// pid it hands out. A pid from a child that has exited could be reused by the
/// time it is checked.
const DEAD_PID: u32 = 0x3FFF_FFFF;

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
    let LiveCheck::Open(live) = live_session(home.path(), SESSION_ID) else {
        panic!("not reported");
    };
    assert_eq!(live.pid, pid);
    assert_eq!(live.entrypoint.as_deref(), Some("cli"));
    // Another session is not.
    assert_eq!(
        live_session(home.path(), "0989e382-5a8f-4fdf-9738-bf02b1f049e1"),
        LiveCheck::NotOpen
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R17
#[test]
fn a_registry_file_left_by_a_process_that_has_exited_is_not_reported() {
    let home = registry_with(DEAD_PID, SESSION_ID);
    assert_eq!(live_session(home.path(), SESSION_ID), LiveCheck::NotOpen);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R17
#[test]
fn session_ids_match_in_either_case_and_a_file_naming_no_session_is_skipped() {
    let pid = std::process::id();
    let home = registry_with(pid, &SESSION_ID.to_uppercase());
    assert!(matches!(
        live_session(home.path(), SESSION_ID),
        LiveCheck::Open(_)
    ));

    // A running process between sessions writes no session id.
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(
        sessions.join(format!("{pid}.json")),
        json!({"pid": pid, "status": "idle"}).to_string(),
    )
    .unwrap();
    assert_eq!(live_session(home.path(), SESSION_ID), LiveCheck::NotOpen);
}

#[test]
fn no_registry_means_nothing_is_open() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(live_session(home.path(), SESSION_ID), LiveCheck::NotOpen);
    // Files that are not registry entries are not read.
    let sessions = home.path().join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join("123.abc.key"), "secret").unwrap();
    assert_eq!(live_session(home.path(), SESSION_ID), LiveCheck::NotOpen);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R17
#[test]
fn a_registry_file_of_another_shape_makes_the_answer_unknown_not_free() {
    // A newer CLI that renamed a field, or a damaged file: either could be
    // the one naming this session, so the check says it could not tell.
    // Named by running pids, so they could be about a live process.
    let pid = std::process::id();
    let home = registry_with(pid, "0989e382-5a8f-4fdf-9738-bf02b1f049e1");
    let sessions = home.path().join("sessions");
    let damaged = sessions.join(format!("{pid}.json"));
    fs::write(&damaged, "not json").unwrap();
    let renamed = sessions.join("other.json");
    fs::write(
        &renamed,
        json!({"pid": "x", "session": SESSION_ID}).to_string(),
    )
    .unwrap();
    let LiveCheck::Unknown { unreadable } = live_session(home.path(), SESSION_ID) else {
        panic!("a file it could not read was taken as not open");
    };
    let mut expected = vec![damaged.clone(), renamed.clone()];
    expected.sort();
    assert_eq!(unreadable, expected);

    // One named by a process that has exited is a leftover, and ignored.
    fs::remove_file(&renamed).unwrap();
    fs::remove_file(&damaged).unwrap();
    fs::write(sessions.join(format!("{DEAD_PID}.json")), "not json").unwrap();
    assert_eq!(live_session(home.path(), SESSION_ID), LiveCheck::NotOpen);

    // A readable file naming the session open still wins.
    let home = registry_with(pid, SESSION_ID);
    fs::write(home.path().join("sessions/damaged.json"), "not json").unwrap();
    assert!(matches!(
        live_session(home.path(), SESSION_ID),
        LiveCheck::Open(_)
    ));
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

// verifies: PROTO-CLAUDE-CODE-STREAM-R17
#[tokio::test]
async fn the_adapter_answers_core_from_the_registry() {
    let pid = std::process::id();
    let transport: Arc<dyn ersa_adapter::Transport> =
        Arc::new(FakeTransport::new(Vec::<String>::new()));
    let ask = |home: &tempfile::TempDir| {
        let adapter = ClaudeAdapter::new().with_claude_home(home.path().to_path_buf());
        let transport = Arc::clone(&transport);
        async move { adapter.open_elsewhere(transport, SESSION_ID).await }
    };
    let open = registry_with(pid, SESSION_ID);
    assert_eq!(ask(&open).await, OpenElsewhere::Yes { pid });

    let free = registry_with(DEAD_PID, SESSION_ID);
    assert_eq!(ask(&free).await, OpenElsewhere::No);

    let unreadable = registry_with(pid, "0989e382-5a8f-4fdf-9738-bf02b1f049e1");
    fs::write(unreadable.path().join("sessions/other.json"), "not json").unwrap();
    assert_eq!(ask(&unreadable).await, OpenElsewhere::Unknown);
}
