//! The driver against stub CLIs built on the fake transport: how it starts the
//! CLI, the `initialize` handshake, and a full `can_use_tool` round trip
//! replayed from fixture 11 (PROTO-CLAUDE-CODE-STREAM, Conformance).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture, is_kind};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterError, Direction, PermissionDecision, SessionConfig};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_protocol::events::PermissionBehavior;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(5);
const PROMPT: &str = "Use the Bash tool to run exactly this command: echo probe > probe.txt. Then report whether it succeeded.";
const PERMISSION_ID: &str = "1ddd13c0-dd2c-4061-a8a8-86574059182e";

fn config() -> SessionConfig {
    SessionConfig {
        cwd: "C:/work".into(),
        permission_mode: "default".into(),
        model: None,
        extra: Value::Null,
    }
}

fn adapter() -> ClaudeAdapter {
    ClaudeAdapter::new()
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
}

fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}"))
}

/// A stub check: the stdin line is a JSON object for which `wanted` holds.
fn expect(
    what: &'static str,
    wanted: impl Fn(&Value) -> bool + Send + Sync + 'static,
) -> impl Fn(&str) -> Result<(), String> + Send + Sync + 'static {
    move |line| {
        let value: Value = serde_json::from_str(line).map_err(|e| format!("{what}: {e}"))?;
        if wanted(&value) {
            Ok(())
        } else {
            Err(format!("expected {what}, got {line}"))
        }
    }
}

fn is_initialize(v: &Value) -> bool {
    v["type"] == "control_request" && v["request"]["subtype"] == "initialize"
}

/// The `initialize` answer from fixture 11, which echoes `ersa-init-1`.
fn init_answer() -> String {
    fixture("11-stream-permission-prompt-tool-stdio").stdout[0].clone()
}

/// A stub that answers `initialize` and then waits for input.
fn ready_stub() -> FakeTransport {
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([init_answer()])
        .hold_open()
}

fn allow() -> PermissionDecision {
    PermissionDecision {
        behavior: PermissionBehavior::Allow,
        message: None,
        updated_input: None,
        apply_suggestions: vec![],
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R01
#[tokio::test]
async fn starts_the_cli_with_every_flag_explicit() {
    let transport = ready_stub();
    let sink = Arc::new(RecordingSink::new());
    let mut config = config();
    config.model = Some("haiku".into());
    config.extra = json!({ "claude_path": "C:/tools/claude.exe" });
    let session = adapter()
        .start(Arc::new(transport.clone()), config, sink.clone())
        .await
        .expect("start");

    let spawned = transport.spawned();
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].program, "C:/tools/claude.exe");
    assert_eq!(spawned[0].cwd.as_deref(), Some("C:/work"));
    assert_eq!(
        spawned[0].args,
        [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "default",
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--permission-prompt-tool",
            "stdio",
            "--await-initialize",
            "--include-partial-messages",
            "--model",
            "haiku",
        ]
    );
    session.stop().await.expect("stop");
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R02
#[tokio::test]
async fn start_returns_only_after_the_initialize_answer_is_recorded() {
    let transport = ready_stub();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");

    // Before anything else can be sent, the log holds the request and its answer.
    let lines = sink.lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].dir, Direction::Host);
    assert!(is_initialize(&parse(&lines[0].line)));
    assert_eq!(lines[1].dir, Direction::Backend);
    assert_eq!(lines[1].line, init_answer());

    session.send_user("hi").await.expect("send");
    assert_eq!(transport.stdin_lines().len(), 2);
    assert!(transport.stub_failures().is_empty());
    session.stop().await.expect("stop");
    sink.wait_for_exit(WAIT).await;
    assert_eq!(sink.exit_codes(), [Some(0)]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R02
#[tokio::test]
async fn a_refused_or_missing_initialize_answer_fails_start() {
    let refused = json!({
        "type": "control_response",
        "response": { "subtype": "error", "request_id": "ersa-init-1", "error": "bad hooks" }
    })
    .to_string();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([refused])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let result = adapter()
        .start(Arc::new(transport), config(), sink.clone())
        .await;
    assert!(matches!(result, Err(AdapterError::Protocol(ref m)) if m.contains("bad hooks")));
    // The half-started CLI was ended and its exit reported.
    assert_eq!(sink.exit_codes().len(), 1);

    // A CLI that exits without answering.
    let transport = FakeTransport::new(["not the answer"]);
    let sink = Arc::new(RecordingSink::new());
    let result = adapter()
        .start(Arc::new(transport), config(), sink.clone())
        .await;
    assert!(matches!(result, Err(AdapterError::Protocol(ref m)) if m.contains("exited")));

    // A CLI that never answers.
    let transport = FakeTransport::new(Vec::<String>::new()).hold_open();
    let sink = Arc::new(RecordingSink::new());
    let result = ClaudeAdapter::new()
        .with_handshake_timeout(Duration::from_millis(50))
        .start(Arc::new(transport), config(), sink.clone())
        .await;
    assert!(matches!(result, Err(AdapterError::Protocol(ref m)) if m.contains("did not answer")));
    assert_eq!(sink.exit_codes(), [None], "killed");
}

/// The round trip the brief's conformance section asks for: a stub CLI built
/// from fixture 11 answers `initialize`, takes the user message, asks to run
/// Bash, checks the answer, and finishes the turn.
// verifies: PROTO-CLAUDE-CODE-STREAM-R03
// verifies: PROTO-CLAUDE-CODE-STREAM-R07
#[tokio::test]
async fn a_permission_round_trips_against_a_stub_of_fixture_11() {
    let fixture = fixture("11-stream-permission-prompt-tool-stdio");
    let out = &fixture.stdout;
    let original_input = parse(&out[4])["request"]["input"].clone();

    let sink = Arc::new(RecordingSink::new());
    // Each check also asserts the line was recorded before the stub could
    // read it (R07: record, then write and flush).
    let recorded = |sink: &Arc<RecordingSink>| {
        let sink = sink.clone();
        move |v: &Value| {
            sink.lines()
                .iter()
                .any(|l| l.dir == Direction::Host && parse(&l.line) == *v)
        }
    };
    let (seen_init, seen_user, seen_answer) = (recorded(&sink), recorded(&sink), recorded(&sink));
    let expected_input = original_input.clone();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("a recorded initialize", move |v| {
            is_initialize(v) && v["request_id"] == "ersa-init-1" && seen_init(v)
        }))
        .then_stdout([&out[0]])
        .wait_for_stdin(expect("the recorded prompt", move |v| {
            v["type"] == "user" && v["message"]["content"][0]["text"] == PROMPT && seen_user(v)
        }))
        .then_stdout(&out[1..=4])
        .wait_for_stdin(expect("a recorded allow echoing the request", move |v| {
            let response = &v["response"];
            v["type"] == "control_response"
                && response["subtype"] == "success"
                && response["request_id"] == PERMISSION_ID
                && response["response"]["behavior"] == "allow"
                && response["response"]["updatedInput"] == expected_input
                && seen_answer(v)
        }))
        .then_stdout(&out[5..])
        .hold_open();

    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    session.send_user(PROMPT).await.expect("send");
    let ask = sink
        .wait_for_line(WAIT, |l| is_kind(l, "control_request/can_use_tool"))
        .await;
    assert_eq!(parse(&ask.line)["request_id"], PERMISSION_ID);
    session
        .decide_permission(PERMISSION_ID, allow())
        .await
        .expect("decide");
    sink.wait_for_line(WAIT, |l| is_kind(l, "result")).await;
    // Answering twice is refused: the request is no longer pending.
    assert!(matches!(
        session.decide_permission(PERMISSION_ID, allow()).await,
        Err(AdapterError::UnknownRequest(_))
    ));
    session.stop().await.expect("stop");
    sink.wait_for_exit(WAIT).await;

    assert_eq!(transport.stub_failures(), Vec::<String>::new());
    // Ersa wrote exactly what the recorded session wrote, byte for byte.
    assert_eq!(transport.stdin_lines(), fixture.sent());
    // And the raw log is the fixture's, in the order it happened: every
    // answer after the line that caused it.
    let recorded: Vec<(Direction, String)> =
        sink.lines().into_iter().map(|l| (l.dir, l.line)).collect();
    let expected: Vec<(Direction, String)> = fixture
        .raw_log()
        .into_iter()
        .map(|l| (l.dir, l.line))
        .collect();
    assert_eq!(recorded, expected);
    assert_eq!(sink.exit_codes(), [Some(0)]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R03
#[tokio::test]
async fn decisions_carry_edits_suggestions_and_denial_messages() {
    let out = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    let second_ask = out[4].replace(PERMISSION_ID, "second");
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([out[0].clone(), out[4].clone(), second_ask])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    sink.wait_for_line(WAIT, |l| l.line.contains("\"second\""))
        .await;

    // Allow with an edited input and the second offered suggestion.
    let mut edited = serde_json::Map::new();
    edited.insert("command".into(), json!("echo edited > probe.txt"));
    let decision = PermissionDecision {
        behavior: PermissionBehavior::Allow,
        message: None,
        updated_input: Some(edited),
        apply_suggestions: vec![1],
    };
    // An index past the offered suggestions is refused, and the request stays.
    let bad = PermissionDecision {
        apply_suggestions: vec![2],
        ..decision.clone()
    };
    assert!(matches!(
        session.decide_permission(PERMISSION_ID, bad).await,
        Err(AdapterError::Protocol(_))
    ));
    session
        .decide_permission(PERMISSION_ID, decision)
        .await
        .expect("allow");

    session
        .decide_permission(
            "second",
            PermissionDecision {
                behavior: PermissionBehavior::Deny,
                message: Some("Not that file.".into()),
                updated_input: None,
                apply_suggestions: vec![],
            },
        )
        .await
        .expect("deny");
    assert!(matches!(
        session.decide_permission("never-asked", allow()).await,
        Err(AdapterError::UnknownRequest(_))
    ));

    let stdin = transport.stdin_lines();
    let allowed = parse(&stdin[1])["response"].clone();
    assert_eq!(allowed["request_id"], PERMISSION_ID);
    assert_eq!(
        allowed["response"],
        json!({
            "behavior": "allow",
            "updatedInput": { "command": "echo edited > probe.txt" },
            "updatedPermissions": [{
                "type": "addDirectories",
                "directories": ["C:\\Users\\user\\AppData\\Local\\Temp\\ersa-rec-zww2Oe"],
                "destination": "session"
            }]
        })
    );
    let denied = parse(&stdin[2])["response"].clone();
    assert_eq!(denied["request_id"], "second");
    assert_eq!(
        denied["response"],
        json!({ "behavior": "deny", "message": "Not that file." })
    );
    session.stop().await.expect("stop");
}

#[tokio::test]
async fn a_prompt_the_cli_withdraws_can_no_longer_be_answered() {
    let out = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    let cancel =
        json!({ "type": "control_cancel_request", "request_id": PERMISSION_ID }).to_string();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([out[0].clone(), out[4].clone(), cancel])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    sink.wait_for_line(WAIT, |l| is_kind(l, "control_cancel_request"))
        .await;

    assert!(matches!(
        session.decide_permission(PERMISSION_ID, allow()).await,
        Err(AdapterError::UnknownRequest(_))
    ));
    // Nothing beyond `initialize` was written.
    assert_eq!(transport.stdin_lines().len(), 1);
    session.stop().await.expect("stop");
}

#[tokio::test]
async fn interrupt_sends_the_interrupt_control_request() {
    let transport = ready_stub();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    session.interrupt().await.expect("interrupt");

    let sent = parse(&transport.stdin_lines()[1]);
    assert_eq!(sent["type"], "control_request");
    assert_eq!(sent["request"], json!({ "subtype": "interrupt" }));
    assert!(
        sent["request_id"]
            .as_str()
            .unwrap()
            .starts_with("ersa-interrupt-")
    );
    session.stop().await.expect("stop");
}

#[tokio::test]
async fn an_unsupported_cli_request_is_refused_rather_than_left_hanging() {
    let hook = json!({
        "type": "control_request",
        "request_id": "cli-7",
        "request": { "subtype": "hook_callback", "callback_id": "x" }
    })
    .to_string();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([init_answer(), hook])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    let refusal = sink
        .wait_for_line(WAIT, |l| {
            l.dir == Direction::Host && l.line.contains("cli-7")
        })
        .await;
    let refusal = parse(&refusal.line);
    assert_eq!(refusal["type"], "control_response");
    assert_eq!(refusal["response"]["subtype"], "error");
    assert_eq!(refusal["response"]["request_id"], "cli-7");
    session.stop().await.expect("stop");
}

#[tokio::test]
async fn after_stop_the_session_is_not_running_and_stderr_is_drained() {
    let transport = ready_stub().with_stderr(["a warning"]);
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport), config(), sink.clone())
        .await
        .expect("start");
    session.stop().await.expect("stop");

    assert_eq!(
        sink.exit_codes(),
        [Some(0)],
        "reported once, before stop returned"
    );
    assert!(matches!(
        session.send_user("too late").await,
        Err(AdapterError::NotRunning)
    ));
    // Stderr is diagnostics, never part of the raw log. It is drained by its
    // own task, which may finish just after the exit.
    tokio::time::timeout(WAIT, async {
        while sink.stderr().is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stderr drained");
    assert_eq!(sink.stderr(), ["a warning"]);
    assert!(sink.lines().iter().all(|l| l.line != "a warning"));
}
