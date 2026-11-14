//! The driver against stub CLIs built on the fake transport: how it starts the
//! CLI, the `initialize` handshake, and a full `can_use_tool` round trip
//! replayed from fixture 11 (PROTO-CLAUDE-CODE-STREAM, Conformance), and
//! questions answered and declined, replayed from the 2.1.316 recordings.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture, is_kind};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{
    Adapter, AdapterError, Direction, PermissionDecision, ProcessSpec, QuestionReply, SessionConfig,
};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_protocol::events::{Answer, AnswerValue, PermissionBehavior};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(5);
const PROMPT: &str = "Use the Bash tool to run exactly this command: echo probe > probe.txt. Then report whether it succeeded.";
const PERMISSION_ID: &str = "1ddd13c0-dd2c-4061-a8a8-86574059182e";

/// A session of a project the user has not said they trust.
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

/// The value the arguments give `flag`, if they name it.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).map(String::as_str)
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
// verifies: PROTO-CLAUDE-CODE-STREAM-R12
#[tokio::test]
async fn starts_the_cli_with_every_flag_explicit() {
    let transport = ready_stub();
    let sink = Arc::new(RecordingSink::new());
    let mut config = config();
    config.model = Some("haiku".into());
    config.project_trusted = true;
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
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "default",
            "--setting-sources",
            "user,project,local",
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

/// `--model` and `--effort` are passed when the session names them and left
/// out otherwise, so the CLI's own defaults apply
/// (PROTO-CLAUDE-CODE-STREAM, Invocation).
// was PROTO-CORE-API-R16; R71 replaces it, unbuilt
#[test]
fn model_and_effort_are_passed_only_when_named() {
    let mut named = config();
    named.model = Some("opus".into());
    named.effort = Some("high".into());
    named.permission_mode = "acceptEdits".into();
    let args = ersa_adapter_claude::invocation_args(&named);
    assert_eq!(flag_value(&args, "--model"), Some("opus"));
    assert_eq!(flag_value(&args, "--effort"), Some("high"));
    assert_eq!(flag_value(&args, "--permission-mode"), Some("acceptEdits"));

    let args = ersa_adapter_claude::invocation_args(&config());
    for flag in ["--model", "--effort"] {
        assert!(!args.iter().any(|a| a == flag), "{flag} in {args:?}");
    }
    assert_eq!(flag_value(&args, "--permission-mode"), Some("default"));
}

/// The efforts and permission modes `backend.list` offers for Claude Code:
/// the brief's effort levels with none marked, as the CLI marks none, and
/// its permission modes, never `bypassPermissions`.
// was PROTO-CORE-API-R16; R71 replaces it, unbuilt
#[test]
fn the_choices_are_the_briefs_efforts_and_permission_modes() {
    let choices = adapter().choices();
    let ids = |list: &[ersa_protocol::core_api::Choice]| -> Vec<String> {
        list.iter().map(|c| c.id.clone()).collect()
    };
    assert_eq!(
        ids(&choices.efforts),
        ["low", "medium", "high", "xhigh", "max"]
    );
    assert!(choices.efforts.iter().all(|c| !c.default), "{choices:?}");
    assert_eq!(
        ids(&choices.permission_modes),
        ["default", "acceptEdits", "plan", "auto"]
    );
    for list in [&choices.efforts, &choices.permission_modes] {
        assert!(list.iter().all(|c| !c.label.is_empty()), "{list:?}");
    }
    // The session's permission mode when none is named is the one that asks
    // before every action outside the allow rules (FLOW-NEW-SESSION-R02).
    assert_eq!(
        ersa_adapter::BackendChoices::default_of(&choices.permission_modes),
        "default"
    );
}

/// Choosing the CLI's own `default` entry passes no `--model`
/// (PROTO-CLAUDE-CODE-STREAM, Invocation).
#[test]
fn the_default_model_passes_no_model_flag() {
    let mut chosen = config();
    chosen.model = Some("default".into());
    let args = ersa_adapter_claude::invocation_args(&chosen);
    assert!(!args.iter().any(|a| a == "--model"), "{args:?}");
}

/// The `initialize` answer of 2.1.316 fixture 25, which lists the models.
fn models_answer() -> String {
    fixture("2.1.316/25-controls-between-turns")
        .stdout
        .into_iter()
        .find(|line| parse(line)["response"]["response"]["models"].is_array())
        .expect("fixture 25 answers initialize with its models")
}

/// A probe CLI that answers `initialize` and exits once its stdin closes.
fn probe_stub(answer: String) -> FakeTransport {
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([answer])
        .hold_open()
}

/// The models are read from the recorded `initialize` answer, every field
/// mapped, in the CLI's order, with `default` on the `default` entry alone
/// and no efforts on a model the CLI gives none; the probe is a new session
/// sent no message, so it leaves no record.
// verifies: PROTO-CLAUDE-CODE-STREAM-R39
#[tokio::test]
async fn models_come_from_the_initialize_answer_of_a_cli_sent_no_message() {
    let transport = probe_stub(models_answer());
    let models = adapter()
        .with_stop_grace(WAIT)
        .models(Arc::new(transport.clone()))
        .await
        .expect("models");

    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "default",
            "opus",
            "claude-fable-5-2",
            "sonnet",
            "haiku",
            "fable",
            "claude-haiku-4-5-20251001",
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-fable-5",
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-opus-4-6",
            "claude-sonnet-4-6",
        ]
    );
    let defaults: Vec<&str> = models
        .iter()
        .filter(|m| m.default)
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(defaults, ["default"]);
    let all = ["low", "medium", "high", "xhigh", "max"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        models[0],
        ersa_protocol::core_api::ModelChoice {
            id: "default".into(),
            label: "Default (recommended)".into(),
            default: true,
            resolved: Some("claude-opus-5-5".into()),
            efforts: Some(all.clone()),
        }
    );
    assert_eq!(models[1].label, "Opus 5.5");
    assert_eq!(models[1].resolved.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(models[5].resolved.as_deref(), Some("claude-fable-5-1"));
    // Haiku 4.5 has no effort levels at all.
    assert_eq!(models[6].label, "Haiku 4.5");
    assert_eq!(models[6].efforts, None);
    assert_eq!(
        models[12].efforts.as_deref(),
        Some(&["low", "medium", "high", "max"].map(String::from)[..])
    );

    // One CLI, started as a new session under an id of its own, with no
    // model, given `initialize` and nothing else.
    let spawned = transport.spawned();
    assert_eq!(spawned.len(), 1);
    assert!(!spawned[0].args.iter().any(|a| a == "--model"));
    assert!(flag_value(&spawned[0].args, "--session-id").is_some());
    assert!(!spawned[0].args.iter().any(|a| a == "--resume"));
    let sent = transport.stdin_lines();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(is_initialize(&parse(&sent[0])));
}

/// A CLI that never answers is one that cannot report its models.
#[tokio::test]
async fn a_cli_that_never_answers_is_an_error() {
    let transport = FakeTransport::new(Vec::<String>::new()).hold_open();
    let error = adapter()
        .with_handshake_timeout(Duration::from_millis(200))
        .models(Arc::new(transport))
        .await
        .expect_err("no answer");
    assert!(matches!(error, AdapterError::Protocol(_)), "{error}");
}

/// A CLI that exits before answering, or answers with no models, is an
/// error too.
#[tokio::test]
async fn a_cli_that_exits_or_lists_nothing_is_an_error() {
    let exits = FakeTransport::new(Vec::<String>::new()).with_exit_code(1);
    let error = adapter().models(Arc::new(exits)).await.expect_err("exited");
    assert!(matches!(error, AdapterError::Protocol(_)), "{error}");

    let empty = probe_stub(init_answer().replace("\"models\"", "\"not_models\""));
    let error = adapter()
        .models(Arc::new(empty))
        .await
        .expect_err("no list");
    assert!(matches!(error, AdapterError::Protocol(_)), "{error}");
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R12
#[test]
fn a_trusted_projects_session_loads_its_settings_and_mcp_configuration() {
    let mut config = config();
    config.project_trusted = true;
    let args = ersa_adapter_claude::invocation_args(&config);
    assert_eq!(
        flag_value(&args, "--setting-sources"),
        Some("user,project,local")
    );
    assert!(!args.iter().any(|a| a == "--strict-mcp-config"), "{args:?}");
}

/// Observed on 2.1.316 against a scratch repository: with `user` alone the
/// CLI loads no `.claude/settings*.json` (so none of their hooks or rules),
/// no `.mcp.json` server even when the user's settings enable them, and no
/// workspace skills, agents or commands, while the user's own settings,
/// hooks and MCP servers still load. `--strict-mcp-config` would drop the
/// user's MCP servers as well, so it stays off.
// verifies: PROTO-CLAUDE-CODE-STREAM-R18
#[test]
fn an_untrusted_projects_session_loads_only_the_users_own_settings() {
    let args = ersa_adapter_claude::invocation_args(&config());
    assert_eq!(flag_value(&args, "--setting-sources"), Some("user"));
    for flag in ["--strict-mcp-config", "--mcp-config", "--settings"] {
        assert!(!args.iter().any(|a| a == flag), "{flag} in {args:?}");
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R13
#[tokio::test]
async fn the_cli_runs_as_entrypoint_ersa_with_inherited_claude_variables_removed() {
    let transport = ready_stub();
    let session = adapter()
        .start(
            Arc::new(transport.clone()),
            config(),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");

    let spec = &transport.spawned()[0];
    assert_eq!(
        spec.env.get("CLAUDE_CODE_ENTRYPOINT").map(String::as_str),
        Some("ersa")
    );
    for name in [
        "CLAUDECODE",
        "CLAUDE_CODE_CHILD_SESSION",
        "NODE_OPTIONS",
        "TRACEPARENT",
        "TRACESTATE",
    ] {
        assert!(spec.env_remove.iter().any(|n| n == name), "{name} kept");
        assert!(!spec.env.contains_key(name), "{name} set");
    }
    session.stop().await.expect("stop");
}

/// The variables that tie a process to another host's session. The
/// transports remove a listed name from whatever the parent passes on, which
/// their own tests show against real processes.
const HOST_SESSION_VARIABLES: [&str; 7] = [
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SSE_PORT",
    "ENABLE_IDE_INTEGRATION",
    "CLAUDE_AGENT_SDK_VERSION",
];

// verifies: PROTO-CLAUDE-CODE-STREAM-R19
#[tokio::test]
async fn the_cli_runs_without_another_hosts_session_variables() {
    let transport = ready_stub();
    let session = adapter()
        .start(
            Arc::new(transport.clone()),
            config(),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");

    let spec = &transport.spawned()[0];
    for name in HOST_SESSION_VARIABLES {
        assert!(spec.env_remove.iter().any(|n| n == name), "{name} kept");
        assert!(!spec.env.contains_key(name), "{name} set");
    }
    // What configures Claude Code itself stays, as in the user's terminal.
    for kept in ["CLAUDE_CONFIG_DIR", "ANTHROPIC_BASE_URL", "PATH"] {
        assert!(!spec.env_remove.iter().any(|n| n == kept), "{kept} removed");
    }
    session.stop().await.expect("stop");
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R13
// verifies: PROTO-CLAUDE-CODE-STREAM-R19
#[test]
fn the_version_check_gets_the_sessions_environment() {
    let mut check = ProcessSpec::default();
    adapter().set_environment(&mut check);
    assert_eq!(
        check.env.get("CLAUDE_CODE_ENTRYPOINT").map(String::as_str),
        Some("ersa")
    );
    for name in HOST_SESSION_VARIABLES.into_iter().chain(["CLAUDECODE"]) {
        assert!(check.env_remove.iter().any(|n| n == name), "{name} kept");
    }
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

    session.send_message(&text("hi")).await.expect("send");
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

/// The `initialize` line the stub received from a session of `config`.
async fn sent_initialize(config: SessionConfig, answer: String) -> Value {
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([answer])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config, sink.clone())
        .await
        .expect("start");
    session.stop().await.expect("stop");
    sink.wait_for_exit(WAIT).await;
    parse(&transport.stdin_lines()[0])
}

/// `init_answer()` with the CLI's verdict on a trust attestation added.
fn init_answer_with_trust(recorded: bool) -> String {
    let mut answer = parse(&init_answer());
    answer["response"]["response"]["workspace_trust_recorded"] = json!(recorded);
    answer.to_string()
}

/// A trusted project's session attests the user's trust for the project's
/// root, which the CLI records for the whole repository, linked worktrees
/// included (observed on 2.1.316).
// verifies: PROTO-CLAUDE-CODE-STREAM-R24
#[tokio::test]
async fn a_trusted_projects_initialize_attests_trust_in_the_projects_root() {
    let mut config = config();
    config.project_trusted = true;
    config.project_path = "C:/projects/app".into();
    let sent = sent_initialize(config, init_answer_with_trust(true)).await;
    assert_eq!(
        sent["request"]["workspaceTrust"],
        json!({ "accepted": true, "directory": "C:/projects/app" })
    );
}

/// An untrusted project's paths are never trusted, whatever path is known.
// verifies: PROTO-CLAUDE-CODE-STREAM-R24
#[tokio::test]
async fn an_untrusted_projects_initialize_attests_nothing() {
    let mut config = config();
    config.project_path = "C:/projects/app".into();
    let sent = sent_initialize(config, init_answer()).await;
    assert!(sent["request"].get("workspaceTrust").is_none(), "{sent}");
}

/// With no project root known there is nothing the user was asked about.
// verifies: PROTO-CLAUDE-CODE-STREAM-R24
#[tokio::test]
async fn a_trusted_session_without_a_project_root_attests_nothing() {
    let mut config = config();
    config.project_trusted = true;
    let sent = sent_initialize(config, init_answer()).await;
    assert!(sent["request"].get("workspaceTrust").is_none(), "{sent}");
}

/// A CLI that declines to record the trust, or says nothing of it, still
/// runs the session; the workspace's always-allow rules just do not apply
/// yet, and Core's log says so.
#[tokio::test]
async fn a_trust_the_cli_did_not_record_is_logged_not_a_failed_start() {
    for (answer, warned) in [
        (init_answer_with_trust(true), false),
        (init_answer_with_trust(false), true),
        (init_answer(), true),
    ] {
        let mut config = config();
        config.project_trusted = true;
        config.project_path = "C:/projects/app".into();
        let log = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let writer = {
            let log = log.clone();
            move || LogWriter(log.clone())
        };
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_ansi(false)
            .finish();
        // The test's runtime has one thread, so the start's handshake logs
        // through this subscriber.
        let _guard = tracing::subscriber::set_default(subscriber);
        sent_initialize(config, answer).await;
        let log = String::from_utf8(log.lock().unwrap().clone()).unwrap();
        assert_eq!(
            log.contains("did not record") && log.contains("C:/projects/app"),
            warned,
            "{log}"
        );
    }
}

/// Collects what the subscriber writes.
struct LogWriter(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The round trip the brief's conformance section asks for: a stub CLI built
/// from fixture 11 answers `initialize`, takes the user message, asks to run
/// Bash, checks the answer, and finishes the turn.
// verifies: PROTO-CLAUDE-CODE-STREAM-R30
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
    session.send_message(&text(PROMPT)).await.expect("send");
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

// verifies: PROTO-CLAUDE-CODE-STREAM-R30
#[tokio::test]
async fn decisions_carry_edits_suggestions_and_denial_messages() {
    let out = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    // A suggestion carrying a field a newer CLI might add: it must come back
    // verbatim in `updatedPermissions`.
    let ask = out[4].replace(
        r#""destination":"session"}]"#,
        r#""destination":"session","addedLater":1}]"#,
    );
    assert_ne!(ask, out[4]);
    let second_ask = out[4].replace(PERMISSION_ID, "second");
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([out[0].clone(), ask, second_ask])
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
                "directories": ["C:/Users/user\\AppData\\Local\\Temp\\ersa-rec-zww2Oe"],
                "destination": "session",
                "addedLater": 1
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

// verifies: PROTO-CLAUDE-CODE-STREAM-R25
#[tokio::test]
async fn a_message_with_attachments_is_sent_as_its_text_its_images_then_its_list() {
    let transport = ready_stub();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    let message = ersa_adapter::OutgoingMessage {
        text: "Look".into(),
        attachments: vec![
            ersa_adapter::OutgoingAttachment::Folder { path: "src".into() },
            ersa_adapter::OutgoingAttachment::Image {
                media_type: "image/png".into(),
                data: "AAAA".into(),
            },
        ],
    };
    session.send_message(&message).await.expect("send");

    let sent = parse(&transport.stdin_lines()[1]);
    assert_eq!(
        sent,
        json!({"type": "user", "message": {"role": "user", "content": [
            {"type": "text", "text": "Look"},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}},
            {"type": "text", "text": "<ersa-attachments>
folder: src
</ersa-attachments>"},
        ]}})
    );
    session.stop().await.expect("stop");
}

#[tokio::test]
async fn a_path_with_a_line_break_is_refused_before_anything_is_written() {
    let transport = ready_stub();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    let written = transport.stdin_lines().len();
    let message = ersa_adapter::OutgoingMessage {
        text: "Look".into(),
        attachments: vec![ersa_adapter::OutgoingAttachment::File {
            path: "a\nb".into(),
        }],
    };
    assert!(matches!(
        session.send_message(&message).await,
        Err(AdapterError::Protocol(_))
    ));
    // Nothing reached the CLI, so Core reads it as not sent.
    assert_eq!(transport.stdin_lines().len(), written);
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
        session.send_message(&text("too late")).await,
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

// ---------------------------------------------------------------------------
// A CLI that will not go quietly

/// Fails the test if `future` takes longer than `WAIT`.
async fn promptly<T>(what: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(WAIT, future)
        .await
        .unwrap_or_else(|_| panic!("{what} did not finish within {WAIT:?}"))
}

#[tokio::test]
async fn the_exit_is_reported_even_when_stdout_never_ends() {
    // A process the agent started holds stdout open after the CLI exits.
    let transport = ready_stub().stdout_outlives_process();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport), config(), sink.clone())
        .await
        .expect("start");

    // Closing stdin ends the CLI; its exit is seen without stdout ending.
    promptly("stop", session.stop()).await.expect("stop");
    assert_eq!(sink.exit_codes(), [Some(0)]);
}

#[tokio::test]
async fn a_kill_is_reported_even_when_stdout_never_ends() {
    // Dropping the session kills the CLI; stdout outlives it.
    let transport = ready_stub().stdout_outlives_process();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport), config(), sink.clone())
        .await
        .expect("start");
    drop(session);
    promptly("the exit after a dropped session", sink.wait_for_exit(WAIT)).await;
    assert_eq!(sink.exit_codes(), [None], "killed");
}

#[tokio::test]
async fn a_failed_start_ends_even_when_stdout_never_ends() {
    let transport = FakeTransport::new(Vec::<String>::new())
        .hold_open()
        .stdout_outlives_process();
    let sink = Arc::new(RecordingSink::new());
    let result = promptly(
        "the failed start",
        ClaudeAdapter::new()
            .with_handshake_timeout(Duration::from_millis(50))
            .start(Arc::new(transport), config(), sink.clone()),
    )
    .await;
    assert!(matches!(result, Err(AdapterError::Protocol(_))));
    assert_eq!(sink.exit_codes(), [None]);
}

#[tokio::test]
async fn stop_kills_a_cli_that_stopped_reading_without_waiting_on_a_stuck_write() {
    // `initialize` fits in the pipe; every later write hangs.
    let transport = ready_stub().wedge_stdin_after(1);
    let sink = Arc::new(RecordingSink::new());
    let session: Arc<dyn ersa_adapter::AdapterSession> = Arc::from(
        adapter()
            .start(Arc::new(transport), config(), sink.clone())
            .await
            .expect("start"),
    );

    let stuck = tokio::spawn({
        let session = session.clone();
        async move { session.send_message(&text("into a full pipe")).await }
    });
    // The write is recorded, then blocks holding the stdin lock.
    sink.wait_for_line(WAIT, |l| l.line.contains("into a full pipe"))
        .await;

    promptly("stop", session.stop()).await.expect("stop");
    assert_eq!(sink.exit_codes(), [None], "killed after the grace period");
    let stuck = promptly("the stuck write", stuck).await.unwrap();
    // Given up on, or failed as the pipe closed: either way it returned.
    assert!(matches!(
        stuck,
        Err(AdapterError::NotRunning | AdapterError::Io(_))
    ));
    assert!(matches!(
        session.send_message(&text("after stop")).await,
        Err(AdapterError::NotRunning)
    ));
}

#[tokio::test]
async fn a_refusal_stuck_on_a_wedged_cli_does_not_stall_the_read_loop() {
    let hook = json!({
        "type": "control_request",
        "request_id": "cli-7",
        "request": { "subtype": "hook_callback" }
    })
    .to_string();
    // The refusal to the hook callback is the write that hangs.
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([init_answer(), hook])
        .hold_open()
        .wedge_stdin_after(1);
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport), config(), sink.clone())
        .await
        .expect("start");
    sink.wait_for_line(WAIT, |l| {
        l.dir == Direction::Host && l.line.contains("cli-7")
    })
    .await;
    promptly("stop", session.stop()).await.expect("stop");
    assert_eq!(sink.exit_codes(), [None]);
}

#[tokio::test]
async fn a_failed_start_quotes_the_cli_stderr() {
    // The CLI rejects its flags and exits without answering.
    let transport = FakeTransport::new(Vec::<String>::new())
        .with_stderr(["error: unknown option '--await-initialize'"])
        .with_exit_code(1);
    let sink = Arc::new(RecordingSink::new());
    let result = adapter()
        .start(Arc::new(transport), config(), sink.clone())
        .await;
    let Err(AdapterError::Protocol(message)) = result else {
        panic!("expected a protocol error");
    };
    assert!(message.contains("exited before answering"), "{message}");
    assert!(
        message.contains("unknown option '--await-initialize'"),
        "{message}"
    );
}

#[tokio::test]
async fn a_missing_cli_says_what_was_tried() {
    let mut config = config();
    config.cwd = String::new();
    config.extra = json!({ "claude_path": "C:/nowhere/claude-missing.exe" });
    let result = adapter()
        .start(
            Arc::new(ersa_adapter::LocalTransport),
            config,
            Arc::new(RecordingSink::new()),
        )
        .await;
    let Err(AdapterError::Io(error)) = result else {
        panic!("expected an I/O error");
    };
    let message = error.to_string();
    assert!(
        message.contains("C:/nowhere/claude-missing.exe"),
        "{message}"
    );
    assert!(message.contains("ERSA_CLAUDE"), "{message}");
}

#[tokio::test]
async fn an_empty_claude_path_counts_as_unset() {
    let transport = ready_stub();
    let mut config = config();
    config.extra = json!({ "claude_path": "" });
    let session = adapter()
        .start(
            Arc::new(transport.clone()),
            config,
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");
    let program = transport.spawned()[0].program.clone();
    let expected = std::env::var("ERSA_CLAUDE")
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "claude".into());
    assert_eq!(program, expected);
    session.stop().await.expect("stop");
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R30
#[tokio::test]
async fn a_prompt_whose_shape_drifted_can_still_be_answered() {
    let drifted = json!({
        "type": "control_request",
        "request_id": "r-9",
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "Bash",
            "input": { "command": "ls" },
            "permission_suggestions": null,
            "decision_reason": { "rule": "ask" }
        }
    })
    .to_string();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([init_answer(), drifted])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    sink.wait_for_line(WAIT, |l| l.line.contains("r-9")).await;

    session
        .decide_permission("r-9", allow())
        .await
        .expect("allow");
    let answer = parse(&transport.stdin_lines()[1]);
    assert_eq!(answer["response"]["request_id"], "r-9");
    assert_eq!(
        answer["response"]["response"],
        json!({ "behavior": "allow", "updatedInput": { "command": "ls" } })
    );
    session.stop().await.expect("stop");
}

#[tokio::test]
async fn one_prompt_is_answered_once_even_by_concurrent_decisions() {
    let out = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    // `initialize` fits; the first answer's write then hangs, holding the
    // prompt reserved while a second decision arrives.
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([out[0].clone(), out[4].clone()])
        .hold_open()
        .wedge_stdin_after(1);
    let sink = Arc::new(RecordingSink::new());
    let session: Arc<dyn ersa_adapter::AdapterSession> = Arc::from(
        adapter()
            .start(Arc::new(transport.clone()), config(), sink.clone())
            .await
            .expect("start"),
    );
    sink.wait_for_line(WAIT, |l| is_kind(l, "control_request/can_use_tool"))
        .await;

    // A decision that cannot be built releases the prompt again.
    let bad = PermissionDecision {
        apply_suggestions: vec![9],
        ..allow()
    };
    assert!(matches!(
        session.decide_permission(PERMISSION_ID, bad).await,
        Err(AdapterError::Protocol(_))
    ));

    let first = tokio::spawn({
        let session = session.clone();
        async move { session.decide_permission(PERMISSION_ID, allow()).await }
    });
    // The first answer is recorded, then stuck writing.
    sink.wait_for_line(WAIT, |l| {
        l.dir == Direction::Host && l.line.contains(PERMISSION_ID)
    })
    .await;
    assert!(matches!(
        session.decide_permission(PERMISSION_ID, allow()).await,
        Err(AdapterError::UnknownRequest(_))
    ));

    promptly("stop", session.stop()).await.expect("stop");
    assert!(
        promptly("the first decision", first)
            .await
            .unwrap()
            .is_err()
    );
    let answers = sink
        .lines()
        .iter()
        .filter(|l| l.dir == Direction::Host && l.line.contains(PERMISSION_ID))
        .count();
    assert_eq!(answers, 1, "only one answer was ever recorded");
}

#[tokio::test]
async fn a_cli_that_never_reads_initialize_fails_start_with_its_stderr() {
    // Stdin is wedged from the first line, so `initialize` is never sent.
    let transport = FakeTransport::new(Vec::<String>::new())
        .with_stderr(["fatal: cannot read input"])
        .hold_open()
        .wedge_stdin_after(0);
    let sink = Arc::new(RecordingSink::new());
    let result = promptly(
        "the failed start",
        ClaudeAdapter::new()
            .with_handshake_timeout(Duration::from_millis(100))
            .start(Arc::new(transport), config(), sink.clone()),
    )
    .await;
    let Err(AdapterError::Protocol(message)) = result else {
        panic!("expected a protocol error");
    };
    assert!(message.contains("initialize"), "{message}");
    assert!(message.contains("fatal: cannot read input"), "{message}");
    assert_eq!(sink.exit_codes(), [None], "killed");
}

/// A message of words alone, as Core sends one (PROTO-CORE-API, Turns and attachments).
fn text(words: &str) -> ersa_adapter::OutgoingMessage {
    ersa_adapter::OutgoingMessage {
        text: words.into(),
        attachments: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Questions (PROTO-CLAUDE-CODE-STREAM, Questions), against stubs of the
// 2.1.316 recordings: the CLI asks three questions (Red or Blue; any of
// Apple, Pear, Plum; Ann or Bob).

fn answer(id: &str, value: AnswerValue, own_words: bool) -> Answer {
    Answer {
        id: id.into(),
        value,
        own_words,
    }
}

/// What the recording's host answered: "Blue", "Apple, Pear", and own words.
fn recorded_answers() -> Vec<Answer> {
    vec![
        answer("0", AnswerValue::One("1".into()), false),
        // Chosen out of order: the CLI still gets them in the options' order.
        answer("1", AnswerValue::Many(vec!["1".into(), "0".into()]), false),
        answer(
            "2",
            AnswerValue::One("Carol, from my own words".into()),
            true,
        ),
    ]
}

/// Plays `name`'s recording against the driver: a stub CLI that answers
/// `initialize`, takes the prompt, asks its question, and waits for an
/// answer equal to the recorded one before finishing the turn. `reply` is
/// how the test answers. Returns what the driver wrote and the recording's
/// host lines.
async fn question_round_trip(name: &str, reply: QuestionReply) -> (Vec<String>, Vec<String>) {
    let fixture = fixture(name);
    let out = fixture.stdout.clone();
    let sent = fixture.sent();
    let ask = out
        .iter()
        .position(|l| parse(l)["request"]["subtype"] == "can_use_tool")
        .unwrap();
    let request_id = parse(&out[ask])["request_id"].as_str().unwrap().to_string();
    let prompt = parse(&sent[1])["message"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    // JSON objects are unordered, so the answer is compared as JSON.
    let recorded_answer = parse(&sent[2]);
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([&out[0]])
        .wait_for_stdin(expect("the recorded prompt", |v| v["type"] == "user"))
        .then_stdout(&out[1..=ask])
        .wait_for_stdin(expect("the recorded answer", move |v| {
            *v == recorded_answer
        }))
        .then_stdout(&out[ask + 1..])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    session.send_user(&prompt).await.expect("send");
    sink.wait_for_line(WAIT, |l| is_kind(l, "control_request/can_use_tool"))
        .await;
    // A question is not a permission request: deciding it is refused.
    assert!(matches!(
        session.decide_permission(&request_id, allow()).await,
        Err(AdapterError::UnknownRequest(_))
    ));
    session
        .answer_question(&request_id, reply.clone())
        .await
        .expect("answer");
    sink.wait_for_line(WAIT, |l| is_kind(l, "result")).await;
    // Answered once; a second answer is refused.
    assert!(matches!(
        session.answer_question(&request_id, reply).await,
        Err(AdapterError::UnknownRequest(_))
    ));
    session.stop().await.expect("stop");
    sink.wait_for_exit(WAIT).await;
    assert_eq!(transport.stub_failures(), Vec::<String>::new());
    (transport.stdin_lines(), sent)
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R29
// verifies: PROTO-CLAUDE-CODE-STREAM-R07
#[tokio::test]
async fn an_answer_is_an_allow_with_the_answers_keyed_by_question_text() {
    let (written, sent) = question_round_trip(
        "2.1.316/07-question-answer",
        QuestionReply::Answers(recorded_answers()),
    )
    .await;
    // Initialize and the prompt as recorded, byte for byte.
    assert_eq!(written[..2], sent[..2]);
    let answer = parse(&written[2]);
    assert_eq!(answer, parse(&sent[2]));
    // Spelled out: the request's input, plus the chosen label, the chosen
    // labels joined by ", " in the options' order, and the own words.
    let response = &answer["response"];
    assert_eq!(response["subtype"], "success");
    assert_eq!(
        response["request_id"],
        "0b943641-f720-4e34-be39-211ee791e85a"
    );
    assert_eq!(response["response"]["behavior"], "allow");
    assert_eq!(
        response["response"]["updatedInput"]["answers"],
        json!({
            "Which color do you want?": "Blue",
            "Which fruits do you want?": "Apple, Pear",
            "Which name do you want?": "Carol, from my own words",
        })
    );
    assert_eq!(written.len(), 3);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R29
#[tokio::test]
async fn a_declined_question_is_a_deny_with_the_brief_s_message() {
    let (written, sent) =
        question_round_trip("2.1.316/09-question-deny", QuestionReply::Declined).await;
    // Exactly what the recorded session wrote, byte for byte.
    assert_eq!(written, sent);
    let response = &parse(&written[2])["response"];
    assert_eq!(
        response["request_id"],
        "d7df0a3d-44a7-4570-a8fe-eb171fd6e8c8"
    );
    assert_eq!(
        response["response"],
        json!({ "behavior": "deny", "message": "The user declined to answer." })
    );
}

/// A stub that asks fixture 07's question and then waits.
async fn asked_question() -> (FakeTransport, Box<dyn ersa_adapter::AdapterSession>, String) {
    let out = fixture("2.1.316/07-question-answer").stdout;
    let request_id = parse(&out[3])["request_id"].as_str().unwrap().to_string();
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([out[0].clone(), out[3].clone()])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    sink.wait_for_line(WAIT, |l| is_kind(l, "control_request/can_use_tool"))
        .await;
    (transport, session, request_id)
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R29
#[tokio::test]
async fn answers_that_do_not_fit_the_question_are_refused_before_anything_is_written() {
    let (transport, session, request_id) = asked_question().await;
    let mut missing = recorded_answers();
    missing.pop();
    let mut bad_option = recorded_answers();
    bad_option[0] = answer("0", AnswerValue::One("7".into()), false);
    let mut list_for_one = recorded_answers();
    list_for_one[0] = answer("0", AnswerValue::Many(vec!["0".into()]), false);
    for answers in [missing, bad_option, list_for_one] {
        assert!(matches!(
            session
                .answer_question(&request_id, QuestionReply::Answers(answers))
                .await,
            Err(AdapterError::Protocol(_))
        ));
    }
    // Only `initialize` was written, and the question can still be answered.
    assert_eq!(transport.stdin_lines().len(), 1);
    session
        .answer_question(&request_id, QuestionReply::Declined)
        .await
        .expect("still pending");
    session.stop().await.expect("stop");
}

#[tokio::test]
async fn a_permission_request_is_not_a_question() {
    let out = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize", is_initialize))
        .then_stdout([out[0].clone(), out[4].clone()])
        .hold_open();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    sink.wait_for_line(WAIT, |l| is_kind(l, "control_request/can_use_tool"))
        .await;
    assert!(matches!(
        session
            .answer_question(PERMISSION_ID, QuestionReply::Declined)
            .await,
        Err(AdapterError::UnknownRequest(_))
    ));
    assert_eq!(transport.stdin_lines().len(), 1);
    session.stop().await.expect("stop");
}
