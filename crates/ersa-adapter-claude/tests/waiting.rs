//! The count of messages a live session's CLI holds behind its running turn,
//! read from the session's record (PROTO-CLAUDE-CODE-STREAM-R46), driven
//! through the adapter against a stub CLI and a scratch Claude home.

mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture, is_kind};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterSink, OutgoingMessage, SessionConfig};
use ersa_adapter_claude::{ClaudeAdapter, record};
use serde_json::{Value, json};
use time::OffsetDateTime;

const WAIT: Duration = Duration::from_secs(5);
const SESSION_ID: &str = "5249e645-e4a0-4c33-9d4a-9b22e2851275";
const CWD: &str = "C:/work";

fn is(subtype: &'static str) -> impl Fn(&str) -> Result<(), String> + Send + Sync + 'static {
    move |line| {
        let value: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
        let kind = value["request"]["subtype"]
            .as_str()
            .or(value["type"].as_str())
            .unwrap_or_default();
        if kind == subtype {
            Ok(())
        } else {
            Err(format!("expected {subtype}, got {line}"))
        }
    }
}

/// An RFC 3339 time with milliseconds, as the CLI stamps its entries.
fn stamp(time: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.millisecond()
    )
}

/// Appends a `queue-operation` entry, as the CLI writes it, stamped `at`.
fn queue(record: &Path, operation: &str, at: &str) {
    let entry = json!({
        "type": "queue-operation",
        "operation": operation,
        "timestamp": at,
        "sessionId": SESSION_ID,
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(record)
        .unwrap();
    writeln!(file, "{entry}").unwrap();
}

fn now() -> String {
    stamp(OffsetDateTime::now_utc())
}

fn message(text: &str) -> OutgoingMessage {
    OutgoingMessage {
        text: text.into(),
        attachments: Vec::new(),
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R46
#[tokio::test]
async fn the_count_is_read_from_the_record_at_a_handover_during_a_turn_and_at_a_turn_start() {
    let stdout = fixture("22-message-during-turn").stdout;
    let (answer, init, result) = (&stdout[0], &stdout[1], &stdout[196]);
    assert!(init.contains(r#""subtype":"init""#) && result.contains(r#""type":"result""#));
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([answer.clone()])
        // The first message starts a turn.
        .wait_for_stdin(is("user"))
        .then_stdout([init.clone()])
        // The second waits behind it; the stub ends the turn and starts the
        // next once the test interrupts, by when the record says it ran.
        .wait_for_stdin(is("user"))
        .wait_for_stdin(is("interrupt"))
        .then_stdout([result.clone(), init.clone()])
        .hold_open();

    let home = tempfile::tempdir().unwrap();
    let path = record::record_path(home.path(), CWD, SESSION_ID);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // An earlier process of the session was stopped with a message waiting:
    // its `enqueue` is not this process's.
    queue(&path, "enqueue", "2026-11-14T01:04:38.253Z");

    let sink = Arc::new(RecordingSink::new());
    let config = SessionConfig {
        cwd: CWD.into(),
        permission_mode: "default".into(),
        resume: Some(SESSION_ID.into()),
        extra: Value::Null,
        ..SessionConfig::default()
    };
    let session = ClaudeAdapter::new()
        .with_claude_home(home.path().to_path_buf())
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
        .start(
            Arc::new(transport),
            config,
            Arc::clone(&sink) as Arc<dyn AdapterSink>,
        )
        .await
        .unwrap();

    // The CLI takes the first message straight into a turn.
    queue(&path, "enqueue", &now());
    queue(&path, "dequeue", &now());
    session.send_message(&message("one")).await.unwrap();
    sink.wait_for_line(WAIT, |l| is_kind(l, "system/init"))
        .await;

    // The second waits. The CLI writes its `enqueue` a moment after the
    // handover, which the read waits for.
    session.send_message(&message("two")).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    queue(&path, "enqueue", &now());
    assert_eq!(sink.wait_for_waiting(WAIT, 1).await, [1]);

    // The turn ends and the CLI takes the waiting message into the next.
    queue(&path, "dequeue", &now());
    session.interrupt().await.unwrap();
    assert_eq!(sink.wait_for_waiting(WAIT, 2).await, [1, 0]);

    session.stop().await.unwrap();
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R46
#[tokio::test]
async fn a_message_folded_into_a_turn_that_uses_tools_is_read_at_the_next_model_call() {
    // Fixture 51: the CLI hands the waiting message to the model at the
    // running turn's next call, which starts no turn of its own.
    let stdout = fixture("51-message-during-tool-turn").stdout;
    let (answer, init) = (&stdout[0], &stdout[1]);
    let requesting = &stdout[19];
    assert!(requesting.contains(r#""status":"requesting""#));
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([answer.clone()])
        .wait_for_stdin(is("user"))
        .then_stdout([init.clone()])
        .wait_for_stdin(is("user"))
        // The test interrupts once the record shows the message taken in;
        // the stub makes its next model call then.
        .wait_for_stdin(is("interrupt"))
        .then_stdout([requesting.clone()])
        .hold_open();

    let home = tempfile::tempdir().unwrap();
    let path = record::record_path(home.path(), CWD, SESSION_ID);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let sink = Arc::new(RecordingSink::new());
    let config = SessionConfig {
        cwd: CWD.into(),
        permission_mode: "default".into(),
        backend_session_id: Some(SESSION_ID.into()),
        extra: Value::Null,
        ..SessionConfig::default()
    };
    let session = ClaudeAdapter::new()
        .with_claude_home(home.path().to_path_buf())
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
        .start(
            Arc::new(transport),
            config,
            Arc::clone(&sink) as Arc<dyn AdapterSink>,
        )
        .await
        .unwrap();

    queue(&path, "enqueue", &now());
    queue(&path, "dequeue", &now());
    session.send_message(&message("one")).await.unwrap();
    sink.wait_for_line(WAIT, |l| is_kind(l, "system/init"))
        .await;
    queue(&path, "enqueue", &now());
    session.send_message(&message("two")).await.unwrap();
    assert_eq!(sink.wait_for_waiting(WAIT, 1).await, [1]);

    queue(&path, "remove", &now());
    session.interrupt().await.unwrap();
    assert_eq!(sink.wait_for_waiting(WAIT, 2).await, [1, 0]);

    session.stop().await.unwrap();
}
