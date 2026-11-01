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
