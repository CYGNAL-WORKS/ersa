//! Naming a new session (PROTO-CLAUDE-CODE-STREAM, Naming a new session):
//! the adapter asks the CLI with `generate_session_title` once a new
//! session's first message with words has gone to it, waits on nothing, and
//! reports the answer as the session's title unless a rename came first. The
//! CLI's side is fixture 2.1.316/43-session-title: `--session-id`, the first
//! message, the request, its answer before the message's echo, a second
//! message.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture, is_kind, project};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{
    Adapter, AdapterSession, Direction, ForkOf, OutgoingAttachment, OutgoingMessage, Projector,
    RawLine, SessionConfig,
};
use ersa_adapter_claude::{ClaudeAdapter, ClaudeProjector};
use serde_json::{Value, json};

const FIXTURE: &str = "2.1.316/43-session-title";
const FIRST: &str = "Why is the sky blue? Answer in one sentence.";
const SECOND: &str = "And why are sunsets red? One sentence.";
const GENERATED: &str = "Why the sky is blue";
const ID: &str = "39885328-1e47-4d9e-804e-5ec43fc6efb1";
const WAIT: Duration = Duration::from_secs(5);

fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

fn adapter() -> ClaudeAdapter {
    ClaudeAdapter::new()
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
}

fn new_session() -> SessionConfig {
    SessionConfig {
        cwd: "C:/work".into(),
        permission_mode: "default".into(),
        backend_session_id: Some(ID.into()),
        ..SessionConfig::default()
    }
}

/// A stub CLI that answers `initialize` as the fixture's CLI did, then holds
/// open without answering anything else: the title request is never
/// answered, so a send that waited on it would never return.
fn silent_cli() -> FakeTransport {
    let stdout = fixture(FIXTURE).stdout;
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|line| match parse(line)["request"]["subtype"].as_str() {
            Some("initialize") => Ok(()),
            _ => Err(format!("expected initialize, got {line}")),
        })
        .then_stdout([&stdout[0]])
        .hold_open()
}

/// Starts a session of `config` on `transport`; returns it and its sink.
async fn start(
    transport: &FakeTransport,
    config: SessionConfig,
) -> (Box<dyn AdapterSession>, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config, sink.clone())
        .await
        .expect("start");
    (session, sink)
}

/// The `generate_session_title` requests written to the CLI so far.
fn title_requests(transport: &FakeTransport) -> Vec<Value> {
    transport
        .stdin_lines()
        .iter()
        .map(|l| parse(l))
        .filter(|l| l["request"]["subtype"] == "generate_session_title")
        .collect()
}

fn text(text: &str) -> OutgoingMessage {
    OutgoingMessage {
        text: text.into(),
        attachments: Vec::new(),
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R49
#[tokio::test]
async fn a_new_session_asks_once_with_its_first_messages_words_as_the_fixture_did() {
    let transport = silent_cli();
    let (session, _sink) = start(&transport, new_session()).await;
    // Neither send waits on the answer, which this CLI never gives.
    tokio::time::timeout(WAIT, session.send_message(&text(FIRST)))
        .await
        .expect("the send does not wait on the title")
        .unwrap();
    tokio::time::timeout(WAIT, session.send_user(SECOND))
        .await
        .expect("the send does not wait on the title")
        .unwrap();
    session.stop().await.unwrap();

    let sent: Vec<Value> = transport.stdin_lines().iter().map(|l| parse(l)).collect();
    let kinds: Vec<String> = sent
        .iter()
        .map(|l| {
            l["request"]["subtype"]
                .as_str()
                .unwrap_or(l["type"].as_str().unwrap())
                .to_string()
        })
        .collect();
    // Right after the first message, and only once.
    assert_eq!(
        kinds,
        ["initialize", "user", "generate_session_title", "user"],
        "{sent:#?}"
    );
    assert_eq!(
        sent[2]["request"],
        json!({ "subtype": "generate_session_title", "description": FIRST, "persist": true })
    );
    // The request the recorded CLI answered had the same shape.
    let recorded = fixture(FIXTURE)
        .sent()
        .iter()
        .map(|l| parse(l))
        .find(|l| l["request"]["subtype"] == "generate_session_title")
        .expect("the fixture sent the request");
    assert_eq!(recorded["request"], sent[2]["request"]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R49
#[tokio::test]
async fn a_message_of_attachments_alone_leaves_the_asking_to_the_next_with_words() {
    let transport = silent_cli();
    let (session, _sink) = start(&transport, new_session()).await;
    let image = OutgoingMessage {
        text: "  ".into(),
        attachments: vec![OutgoingAttachment::Image {
            media_type: "image/png".into(),
            data: "iVBORw0KGgo=".into(),
        }],
    };
    session.send_message(&image).await.unwrap();
    assert!(title_requests(&transport).is_empty());
    session.send_user(SECOND).await.unwrap();
    session.stop().await.unwrap();
    let asked = title_requests(&transport);
    assert_eq!(asked.len(), 1, "{asked:#?}");
    assert_eq!(asked[0]["request"]["description"], SECOND);
    assert_eq!(asked[0]["request"]["persist"], true);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R49
#[tokio::test]
async fn a_resumed_session_or_a_fork_asks_nothing() {
    let resumed = SessionConfig {
        backend_session_id: None,
        resume: Some(ID.into()),
        ..new_session()
    };
    let fork = SessionConfig {
        backend_session_id: Some("0b5f2c4e-8c1a-4b7e-9d2f-3a6b7c8d9e0f".into()),
        fork_of: Some(ForkOf {
            original: ID.into(),
            name: None,
        }),
        ..new_session()
    };
    // Nor a session the CLI names itself, which the adapter did not start
    // under an id of its own.
    let unnamed = SessionConfig {
        backend_session_id: None,
        ..new_session()
    };
    for config in [resumed, fork, unnamed] {
        let transport = silent_cli();
        let (session, _sink) = start(&transport, config.clone()).await;
        session.send_user(FIRST).await.unwrap();
        session.send_user(SECOND).await.unwrap();
        session.stop().await.unwrap();
        assert!(
            title_requests(&transport).is_empty(),
            "{config:?}: {:#?}",
            transport.stdin_lines()
        );
    }
}

// ---------------------------------------------------------------------------
// The answer (R50), over the fixture's raw log and variations of it.

/// The title the projector reports after `lines`.
fn title_after(lines: &[RawLine]) -> Option<String> {
    let mut projector = ClaudeProjector::new();
    for line in lines {
        projector.project(line);
    }
    projector
        .announced_title()
        .map(|(_, title)| title.to_string())
}

fn recorded_log() -> Vec<RawLine> {
    fixture(FIXTURE).raw_log()
}

/// Where in `lines` the title request and its answer are.
fn request_and_answer(lines: &[RawLine]) -> (usize, usize) {
    let request = lines
        .iter()
        .position(|l| is_kind(l, "control_request/generate_session_title"))
        .expect("the request");
    let id = parse(&lines[request].line)["request_id"].clone();
    let answer = lines
        .iter()
        .position(|l| {
            let v = parse(&l.line);
            v["type"] == "control_response" && v["response"]["request_id"] == id
        })
        .expect("its answer");
    (request, answer)
}

/// `lines` with the answer's `response` replaced by `response`.
fn answered_with(mut lines: Vec<RawLine>, response: Value) -> Vec<RawLine> {
    let (_, answer) = request_and_answer(&lines);
    let mut line = parse(&lines[answer].line);
    line["response"] = response;
    lines[answer].line = line.to_string();
    lines
}

/// `lines` with `extra` inserted before index `at`, renumbered.
fn with_line_at(mut lines: Vec<RawLine>, at: usize, dir: Direction, extra: Value) -> Vec<RawLine> {
    let ts = lines[at].ts.clone();
    lines.insert(
        at,
        RawLine {
            index: 0,
            ts,
            dir,
            line: extra.to_string(),
        },
    );
    for (index, line) in lines.iter_mut().enumerate() {
        line.index = index as u64;
    }
    lines
}

/// `lines` with the request stamped `asked` and its answer `answered`.
fn timed(mut lines: Vec<RawLine>, asked: &str, answered: &str) -> Vec<RawLine> {
    let (request, answer) = request_and_answer(&lines);
    lines[request].ts = asked.into();
    lines[answer].ts = answered.into();
    lines
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R50
#[test]
fn the_recorded_answer_is_the_sessions_title_and_nothing_else_changes() {
    let lines = recorded_log();
    assert_eq!(title_after(&lines).as_deref(), Some(GENERATED));
    // The request and its answer project as passthroughs only: no error, and
    // the two messages and their turns project as they would without them.
    let without: Vec<RawLine> = {
        let (request, answer) = request_and_answer(&lines);
        let mut kept: Vec<RawLine> = lines
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != request && *i != answer)
            .map(|(_, l)| l.clone())
            .collect();
        for (index, line) in kept.iter_mut().enumerate() {
            line.index = index as u64;
        }
        kept
    };
    let not_passthrough = |lines: &[RawLine]| -> Vec<Value> {
        project(lines)
            .into_iter()
            .map(|(_, p)| serde_json::to_value(&p.event).unwrap())
            .filter(|e| e["type"] != "backend.passthrough")
            .collect()
    };
    assert_eq!(not_passthrough(&lines), not_passthrough(&without));
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R50
#[test]
fn a_rename_sent_or_announced_before_the_answer_wins() {
    let (request, _) = request_and_answer(&recorded_log());
    let rename = json!({"type": "control_request", "request_id": "ersa-rename-9",
        "request": {"subtype": "rename_session", "title": "Mine", "source": "host", "session_id": ID}});
    let sent = with_line_at(recorded_log(), request, Direction::Host, rename);
    assert_eq!(title_after(&sent), None);

    let announced = json!({"type": "system", "subtype": "session_title_changed", "title": "Mine",
        "session_id": ID, "uuid": "00000000-0000-4000-8000-000000000000"});
    let seen = with_line_at(
        recorded_log(),
        request,
        Direction::Backend,
        announced.clone(),
    );
    assert_eq!(title_after(&seen).as_deref(), Some("Mine"));

    // A rename announced after the answer is the title from then on.
    let (_, answer) = request_and_answer(&recorded_log());
    let later = with_line_at(recorded_log(), answer + 1, Direction::Backend, announced);
    assert_eq!(title_after(&later).as_deref(), Some("Mine"));
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R50
#[test]
fn a_decline_an_error_or_no_title_names_nothing_and_says_nothing() {
    let error = json!({"subtype": "error", "request_id": "ersa-generate_session_title-1",
        "error": "Unsupported control request subtype: generate_session_title"});
    let no_title = json!({"subtype": "success", "request_id": "ersa-generate_session_title-1",
        "response": {}});
    let empty_title = json!({"subtype": "success", "request_id": "ersa-generate_session_title-1",
        "response": {"title": ""}});
    let unknown = json!({"subtype": "declined", "request_id": "ersa-generate_session_title-1"});
    for response in [error, no_title, empty_title, unknown] {
        let lines = answered_with(recorded_log(), response.clone());
        assert_eq!(title_after(&lines), None, "{response}");
        let errors: Vec<Value> = project(&lines)
            .into_iter()
            .map(|(_, p)| serde_json::to_value(&p.event).unwrap())
            .filter(|e| e["type"] == "error" || e["type"] == "session.notice")
            .collect();
        assert!(errors.is_empty(), "{response}: {errors:#?}");
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R49
// verifies: PROTO-CLAUDE-CODE-STREAM-R50
#[test]
fn an_answer_past_the_five_minute_deadline_names_nothing() {
    let asked = "2026-11-14T10:00:00.000Z";
    let in_time = timed(recorded_log(), asked, "2026-11-14T10:05:00.000Z");
    assert_eq!(title_after(&in_time).as_deref(), Some(GENERATED));
    let late = timed(recorded_log(), asked, "2026-11-14T10:05:00.001Z");
    assert_eq!(title_after(&late), None);
}
