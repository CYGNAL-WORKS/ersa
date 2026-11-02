//! `ersa-core replay` end to end: the binary serves a recorded session that a
//! client can list and subscribe to, and stops on `core.shutdown`.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const WAIT: Duration = Duration::from_secs(60);

// verifies: PROTO-CORE-API-R09
#[tokio::test]
async fn replay_serves_a_recorded_session_to_list_and_subscribe_to() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits two levels below the workspace root")
        .join("docs/briefs/protocol/fixtures/claude-code/2.1.302/11-stream-permission-prompt-tool-stdio.jsonl");
    let label = format!("replay-test-{}", std::process::id());
    let endpoint = ersa_core::platform::labelled_endpoint(&label).unwrap();
    let mut core = tokio::process::Command::new(env!("CARGO_BIN_EXE_ersa-core"))
        .args([
            "replay",
            "--pace",
            "instant",
            "--endpoint",
            &label,
            "--fixture",
        ])
        .arg(&fixture)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("ersa-core starts");

    let mut stream = None;
    for _ in 0..300 {
        if let Ok(connected) = ersa_core::connect(&endpoint).await {
            stream = Some(connected);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let (read, mut write) = tokio::io::split(stream.expect("the replay server is listening"));
    let mut lines = BufReader::new(read).lines();
    let mut next_id = 0;
    let mut call = async |method: &str, params: Value| {
        next_id += 1;
        let request =
            json!({ "jsonrpc": "2.0", "id": next_id, "method": method, "params": params });
        write
            .write_all(format!("{request}\n").as_bytes())
            .await
            .expect("write");
        next_id
    };
    let mut events = Vec::new();
    let read_until = async |lines: &mut tokio::io::Lines<_>, id: i64, events: &mut Vec<Value>| loop {
        let line = tokio::time::timeout(WAIT, lines.next_line())
            .await
            .expect("the replay server answered in time")
            .expect("read")
            .expect("the connection stays open");
        let line: Value = serde_json::from_str(&line).expect("JSON");
        if line["id"] == id {
            return line["result"].clone();
        }
        if line["method"] == "session.event" {
            events.push(line["params"]["event"]["event"].clone());
        }
    };

    let id = call(
        "core.hello",
        json!({ "client": "replay-test", "client_version": "0", "api_versions": [2] }),
    )
    .await;
    read_until(&mut lines, id, &mut events).await;
    let id = call("session.list", json!({})).await;
    let sessions = read_until(&mut lines, id, &mut events).await;
    assert_eq!(sessions.as_array().map(Vec::len), Some(1), "{sessions:#}");
    assert_eq!(
        sessions[0]["title"],
        "11-stream-permission-prompt-tool-stdio"
    );
    let id = call(
        "session.subscribe",
        json!({ "id": sessions[0]["id"], "from_seq": 0 }),
    )
    .await;
    read_until(&mut lines, id, &mut events).await;
    // Read on until the recorded turn has completed.
    while !events.iter().any(|e| e["type"] == "turn.completed") {
        let line = tokio::time::timeout(WAIT, lines.next_line())
            .await
            .expect("events arrive")
            .expect("read")
            .expect("the connection stays open");
        let line: Value = serde_json::from_str(&line).expect("JSON");
        if line["method"] == "session.event" {
            events.push(line["params"]["event"]["event"].clone());
        }
    }
    assert!(events.iter().any(|e| e["type"] == "permission.requested"));
    assert!(events.iter().any(|e| e["type"] == "permission.decided"));

    let id = call("core.shutdown", json!({})).await;
    read_until(&mut lines, id, &mut events).await;
    let status = tokio::time::timeout(WAIT, core.wait())
        .await
        .expect("the replay server exited")
        .expect("wait");
    assert!(status.success());
}

/// Writes `stdout` and `meta` as a recording in `dir`; returns the stdout file.
fn write_recording(dir: &Path, stdout: &str, meta: Value) -> std::path::PathBuf {
    let jsonl = dir.join("rec.jsonl");
    std::fs::write(&jsonl, stdout).unwrap();
    std::fs::write(dir.join("rec.meta.json"), meta.to_string()).unwrap();
    jsonl
}

/// Runs `ersa-core replay` on `fixture` and expects it to refuse at start,
/// returning what it wrote to stderr.
async fn refused_at_start(fixture: &Path) -> String {
    let label = format!("replay-refused-{}", std::process::id());
    let run = tokio::process::Command::new(env!("CARGO_BIN_EXE_ersa-core"))
        .args([
            "replay",
            "--pace",
            "instant",
            "--endpoint",
            &label,
            "--fixture",
        ])
        .arg(fixture)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(WAIT, run)
        .await
        .expect("a refused replay exits")
        .expect("ersa-core runs");
    assert!(!out.status.success(), "the replay is refused");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"s"}"#;

// Conformance: a recording the replay server cannot read faithfully is
// refused at start with the reason, never played with a default.
#[tokio::test]
async fn replay_refuses_empty_timings_for_stdout_lines() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = write_recording(
        dir.path(),
        INIT,
        json!({ "claude_code_version": "2.1.302", "stdout_timing_ms": [] }),
    );
    let stderr = refused_at_start(&fixture).await;
    assert!(
        stderr.contains("0 stdout timings for 1 stdout lines"),
        "{stderr}"
    );
}

#[tokio::test]
async fn replay_refuses_a_recording_without_a_version() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = write_recording(dir.path(), INIT, json!({ "stdout_timing_ms": [0] }));
    let stderr = refused_at_start(&fixture).await;
    assert!(stderr.contains("claude_code_version"), "{stderr}");
    // An empty version is no version either.
    let fixture = write_recording(
        dir.path(),
        INIT,
        json!({ "claude_code_version": "", "stdout_timing_ms": [0] }),
    );
    let stderr = refused_at_start(&fixture).await;
    assert!(stderr.contains("claude_code_version"), "{stderr}");
}

// Ctrl+C during start-up, before the server binds, still removes the
// replay's temporary data directory. Unix only: a test cannot send one
// process alone a Ctrl+C on Windows. TMPDIR points the replay at a
// directory the test watches.
#[cfg(unix)]
#[tokio::test]
async fn ctrl_c_during_start_up_removes_the_data_directory() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("docs/briefs/protocol/fixtures/claude-code/2.1.302/11-stream-permission-prompt-tool-stdio.jsonl");
    let tmp = tempfile::tempdir().unwrap();
    let label = format!("replay-ctrlc-{}", std::process::id());
    let mut core = tokio::process::Command::new(env!("CARGO_BIN_EXE_ersa-core"))
        .args([
            "replay",
            "--pace",
            "instant",
            "--endpoint",
            &label,
            "--fixture",
        ])
        .arg(&fixture)
        .env("TMPDIR", tmp.path())
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("ersa-core starts");
    // Interrupt as soon as the data directory exists: the server is then
    // at most part way through binding.
    let created = async {
        while std::fs::read_dir(tmp.path()).unwrap().next().is_none() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    tokio::time::timeout(WAIT, created)
        .await
        .expect("the replay creates its data directory");
    let pid = core.id().unwrap().to_string();
    let kill = std::process::Command::new("kill")
        .args(["-INT", &pid])
        .status()
        .unwrap();
    assert!(kill.success());
    tokio::time::timeout(WAIT, core.wait())
        .await
        .expect("the replay stops on Ctrl+C")
        .unwrap();
    let left: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}
