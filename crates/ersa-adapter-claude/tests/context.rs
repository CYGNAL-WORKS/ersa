//! The context ring's reading (PROTO-CLAUDE-CODE-STREAM-R71 to R74): the
//! context in use while a turn runs, from each main-thread `assistant` line,
//! and the CLI's own count and window from `get_context_usage`, which the
//! adapter asks for when the CLI has answered `initialize`, after each
//! `set_model` and after each `compact_boundary`. The CLI's side is 2.1.316
//! fixtures `44-context-usage`, `45-context-usage-resumed` and
//! `46-context-usage-during-compact`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, events, fixture, log, project};
use ersa_adapter::Direction::{Backend, Host};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterSession, RawLine, SessionConfig, SessionControl};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_protocol::events::*;
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(5);

fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap_or(Value::Null)
}

/// The `usage.updated` events of a projection, with the raw line each came from.
fn usage_updates(lines: &[RawLine]) -> Vec<(u64, UsageUpdated)> {
    project(lines)
        .into_iter()
        .filter_map(|(raw, p)| match p.event {
            Event::UsageUpdated(u) => Some((raw, u)),
            _ => None,
        })
        .collect()
}

/// The sum of an `assistant` line's `message.usage` counts.
fn usage_sum(line: &Value) -> u64 {
    let usage = &line["message"]["usage"];
    [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
        "output_tokens",
    ]
    .iter()
    .filter_map(|k| usage[*k].as_u64())
    .sum()
}

// ---------------------------------------------------------------------------
// R71: the context while a turn runs

/// Each main-thread reply reports what the model holds after its call, so a
/// session has a reading before its first turn's `result`.
// verifies: PROTO-CLAUDE-CODE-STREAM-R71
#[test]
fn each_main_thread_reply_reports_its_calls_context_before_the_turn_ends() {
    let raw = fixture("2.1.316/00-session-tools").raw_log();
    let updates = usage_updates(&raw);
    let first_result = raw
        .iter()
        .find(|l| l.dir == Backend && parse(&l.line)["type"] == "result")
        .expect("a result")
        .index;

    let mut replies = 0;
    for line in raw.iter().filter(|l| l.dir == Backend) {
        let value = parse(&line.line);
        if value["type"] != "assistant" {
            continue;
        }
        replies += 1;
        let context: Vec<Option<u64>> = updates
            .iter()
            .filter(|(raw, _)| *raw == line.index)
            .map(|(_, u)| u.context_tokens)
            .collect();
        assert_eq!(
            context,
            [Some(usage_sum(&value))],
            "raw line {}",
            line.index
        );
    }
    assert!(replies >= 5, "the fixture's replies were all checked");

    // The first reply: 2 in, 12,905 cache writes, 31,301 cache reads, 4 out,
    // reported before the turn's `result`.
    let (raw_at, first) = updates
        .iter()
        .find(|(_, u)| u.context_tokens.is_some())
        .expect("a reading");
    assert!(*raw_at < first_result);
    assert_eq!(first.context_tokens, Some(44_212));
}

/// A subagent's lines count the subagent's own context and move nothing.
// verifies: PROTO-CLAUDE-CODE-STREAM-R71
#[test]
fn a_subagents_replies_report_no_context() {
    let raw = fixture("2.1.316/34-tool-agent").raw_log();
    let updates = usage_updates(&raw);
    let subagent: Vec<&RawLine> = raw
        .iter()
        .filter(|l| {
            let v = parse(&l.line);
            l.dir == Backend && v["type"] == "assistant" && v["parent_tool_use_id"].is_string()
        })
        .collect();
    assert!(!subagent.is_empty(), "the fixture has a subagent's replies");
    for line in subagent {
        assert!(
            updates
                .iter()
                .filter(|(raw, _)| *raw == line.index)
                .all(|(_, u)| u.context_tokens.is_none()),
            "raw line {}",
            line.index
        );
    }
    // The main thread's replies in the same session still report.
    assert!(updates.iter().any(|(_, u)| u.context_tokens.is_some()));
}

/// A message the CLI wrote itself, and a reply whose counts sum to nothing,
/// report no context.
// verifies: PROTO-CLAUDE-CODE-STREAM-R71
#[test]
fn a_synthetic_reply_or_one_counting_nothing_reports_no_context() {
    let reply = |model: &str, usage: Value| {
        json!({"type": "assistant", "parent_tool_use_id": null, "uuid": "u1", "session_id": "s",
            "message": {"id": "m1", "type": "message", "role": "assistant", "model": model,
                "content": [{"type": "text", "text": "hi"}], "usage": usage}})
        .to_string()
    };
    let counted = json!({"input_tokens": 5, "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 100, "output_tokens": 0});
    let zero = json!({"input_tokens": 0, "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": 0, "output_tokens": 0});
    let context = |line: String| -> Vec<Option<u64>> {
        usage_updates(&log(&[(Backend, &line)]))
            .into_iter()
            .map(|(_, u)| u.context_tokens)
            .filter(Option::is_some)
            .collect()
    };
    assert_eq!(
        context(reply("claude-opus-5-5", counted.clone())),
        [Some(105)]
    );
    assert!(context(reply("<synthetic>", counted)).is_empty());
    assert!(context(reply("claude-opus-5-5", zero)).is_empty());
}

// ---------------------------------------------------------------------------
// R72 to R74: the CLI's answer

/// The answers in the recording, before the first turn, after it, after a
/// model change and after a compaction, each give the count and the window.
// verifies: PROTO-CLAUDE-CODE-STREAM-R72
#[test]
fn each_get_context_usage_answer_projects_its_count_and_window() {
    let raw = fixture("2.1.316/44-context-usage").raw_log();
    let answers: Vec<u64> = raw
        .iter()
        .filter(|l| {
            l.dir == Backend
                && parse(&l.line)["response"]["request_id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("ersa-get_context_usage-"))
        })
        .map(|l| l.index)
        .collect();
    let from_answers: Vec<(Option<u64>, Option<u64>)> = usage_updates(&raw)
        .into_iter()
        .filter(|(raw, _)| answers.contains(raw))
        .map(|(_, u)| (u.context_tokens, u.context_window))
        .collect();
    assert_eq!(
        from_answers,
        [
            (Some(32_983), Some(1_000_000)),
            (Some(44_225), Some(1_000_000)),
            (Some(44_371), Some(1_000_000)),
            (Some(46_460), Some(1_000_000)),
        ]
    );

    // A resumed session's whole conversation, before any message.
    let resumed = fixture("2.1.316/45-context-usage-resumed").raw_log();
    let first = usage_updates(&resumed)
        .into_iter()
        .next()
        .expect("a reading");
    assert_eq!(
        (first.1.context_tokens, first.1.context_window),
        (Some(44_974), Some(1_000_000))
    );
}

/// `rawMaxTokens` is the window; `maxTokens` stands in when it is absent.
// verifies: PROTO-CLAUDE-CODE-STREAM-R72
#[test]
fn the_window_is_raw_max_tokens_else_max_tokens() {
    let reading = |answer: Value| {
        let request = r#"{"type":"control_request","request_id":"ersa-context-2","request":{"subtype":"get_context_usage"}}"#;
        let answer = json!({"type": "control_response", "response": {"subtype": "success",
            "request_id": "ersa-context-2", "response": answer}})
        .to_string();
        usage_updates(&log(&[(Host, request), (Backend, &answer)]))
            .into_iter()
            .map(|(_, u)| (u.context_tokens, u.context_window))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        reading(json!({"totalTokens": 10, "rawMaxTokens": 200_000, "maxTokens": 180_000})),
        [(Some(10), Some(200_000))]
    );
    assert_eq!(
        reading(json!({"totalTokens": 10, "maxTokens": 180_000})),
        [(Some(10), Some(180_000))]
    );
}

/// A refused request, and an answer with neither figure, project nothing:
/// the session's reading and state stay as they were.
// verifies: PROTO-CLAUDE-CODE-STREAM-R73
#[test]
fn a_failed_or_empty_answer_projects_nothing() {
    let request = r#"{"type":"control_request","request_id":"ersa-context-2","request":{"subtype":"get_context_usage"}}"#;
    let refused = r#"{"type":"control_response","response":{"subtype":"error","request_id":"ersa-context-2","error":"no"}}"#;
    let empty = r#"{"type":"control_response","response":{"subtype":"success","request_id":"ersa-context-2","response":{"totalTokens":0,"categories":[]}}}"#;
    for answer in [refused, empty] {
        let projected = events(&log(&[(Host, request), (Backend, answer)]));
        assert!(
            projected
                .iter()
                .all(|e| matches!(e, Event::BackendPassthrough(_))),
            "{answer}: {projected:#?}"
        );
    }
}

/// A turn's end, with `modelUsage` naming a window.
fn result_line(window: u64) -> String {
    json!({"type": "result", "subtype": "success", "is_error": false, "duration_ms": 1,
        "num_turns": 1, "result": "ok", "session_id": "s", "total_cost_usd": 0.1,
        "usage": {"input_tokens": 1, "output_tokens": 1},
        "modelUsage": {"claude-opus-5-5": {"contextWindow": window},
            "claude-haiku-5-5": {"contextWindow": window}}})
    .to_string()
}

fn init_line(model: &str) -> String {
    json!({"type": "system", "subtype": "init", "session_id": "s", "model": model,
        "cwd": "C:/work", "tools": [], "permissionMode": "default"})
    .to_string()
}

const CONTEXT_REQUEST: &str = r#"{"type":"control_request","request_id":"ersa-context-2","request":{"subtype":"get_context_usage"}}"#;

fn context_answer(model: &str, window: u64) -> String {
    json!({"type": "control_response", "response": {"subtype": "success",
        "request_id": "ersa-context-2", "response": {"totalTokens": 30_000,
            "rawMaxTokens": window, "maxTokens": window, "model": model}}})
    .to_string()
}

/// The windows `result` lines report, in order.
fn result_windows(lines: &[RawLine]) -> Vec<Option<u64>> {
    let results: Vec<u64> = lines
        .iter()
        .filter(|l| parse(&l.line)["type"] == "result")
        .map(|l| l.index)
        .collect();
    usage_updates(lines)
        .into_iter()
        .filter(|(raw, _)| results.contains(raw))
        .map(|(_, u)| u.context_window)
        .collect()
}

/// After the CLI has answered for the current model, `result` leaves its
/// window alone; until it has, and again after a model change it has not
/// answered for, `modelUsage` gives it.
// verifies: PROTO-CLAUDE-CODE-STREAM-R74
#[test]
fn model_usage_gives_the_window_only_while_no_answer_has_for_the_current_model() {
    let opus = init_line("claude-opus-5-5");
    let answer = context_answer("claude-opus-5-5", 1_000_000);
    let result = result_line(200_000);
    let set_model = r#"{"type":"control_request","request_id":"ersa-set_model-3","request":{"subtype":"set_model","model":"haiku"}}"#;
    let set_model_answer = r#"{"type":"control_response","response":{"subtype":"success","request_id":"ersa-set_model-3","response":{"model":"claude-haiku-5-5"}}}"#;
    let user =
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#;

    // No answer yet: modelUsage fills in.
    let unanswered = log(&[(Host, user), (Backend, &opus), (Backend, &result)]);
    assert_eq!(result_windows(&unanswered), [Some(200_000)]);

    // Answered for the session's model: the answer's window stands.
    let answered = log(&[
        (Host, CONTEXT_REQUEST),
        (Backend, &answer),
        (Host, user),
        (Backend, &opus),
        (Backend, &result),
    ]);
    assert_eq!(result_windows(&answered), [None]);

    // A refused request leaves modelUsage to fill in.
    let refused = r#"{"type":"control_response","response":{"subtype":"error","request_id":"ersa-context-2","error":"no"}}"#;
    let failed = log(&[
        (Host, CONTEXT_REQUEST),
        (Backend, refused),
        (Host, user),
        (Backend, &opus),
        (Backend, &result),
    ]);
    assert_eq!(result_windows(&failed), [Some(200_000)]);

    // The model changed and its answer has not come: modelUsage again.
    let changed = log(&[
        (Host, CONTEXT_REQUEST),
        (Backend, &answer),
        (Host, set_model),
        (Backend, set_model_answer),
        (Host, user),
        (Backend, &init_line("claude-haiku-5-5")),
        (Backend, &result),
    ]);
    assert_eq!(result_windows(&changed), [Some(200_000)]);

    // In the recording every turn ends after an answer for its model.
    let recorded = fixture("2.1.316/44-context-usage").raw_log();
    assert_eq!(result_windows(&recorded), [None, None]);
}

// ---------------------------------------------------------------------------
// R72, R73: the driver asks

fn adapter() -> ClaudeAdapter {
    ClaudeAdapter::new()
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
}

fn config() -> SessionConfig {
    SessionConfig {
        cwd: "C:/work".into(),
        permission_mode: "default".into(),
        ..SessionConfig::default()
    }
}

fn init_answer() -> String {
    fixture("2.1.316/44-context-usage").stdout[0].clone()
}

fn expect(subtype: &'static str) -> impl Fn(&str) -> Result<(), String> + Send + Sync {
    move |line| match parse(line)["request"]["subtype"].as_str() {
        Some(s) if s == subtype => Ok(()),
        _ => Err(format!("expected {subtype}, got {line}")),
    }
}

/// The subtypes of the control requests and the types of other lines
/// written to the CLI so far.
fn written(transport: &FakeTransport) -> Vec<String> {
    transport
        .stdin_lines()
        .iter()
        .map(|l| {
            let v = parse(l);
            v["request"]["subtype"]
                .as_str()
                .or(v["type"].as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

async fn start(transport: &FakeTransport) -> (Box<dyn AdapterSession>, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(Arc::new(transport.clone()), config(), sink.clone())
        .await
        .expect("start");
    (session, sink)
}

/// The request goes out once `initialize` is answered, before anything
/// else, as the recording sent it.
// verifies: PROTO-CLAUDE-CODE-STREAM-R72
#[tokio::test]
async fn the_cli_is_asked_once_initialize_is_answered() {
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize"))
        .then_stdout([init_answer()])
        .hold_open();
    let (session, _sink) = start(&transport).await;
    assert_eq!(written(&transport), ["initialize", "get_context_usage"]);
    let request = parse(&transport.stdin_lines()[1]);
    let recorded = fixture("2.1.316/44-context-usage")
        .sent()
        .iter()
        .map(|l| parse(l))
        .find(|l| l["request"]["subtype"] == "get_context_usage")
        .expect("the recording asked");
    assert_eq!(request["request"], recorded["request"]);
    assert_eq!(request["request"], json!({"subtype": "get_context_usage"}));
    session.stop().await.unwrap();
}

/// After the CLI answers `set_model`, it is asked again; a refused
/// `set_model` changes no model and asks nothing.
// verifies: PROTO-CLAUDE-CODE-STREAM-R72
#[tokio::test]
async fn the_cli_is_asked_again_after_it_answers_set_model() {
    let answer = |subtype: &str| {
        json!({"type": "control_response", "response": {"subtype": subtype,
            "request_id": "ersa-set_model-3", "response": {"model": "claude-haiku-5-5"},
            "error": "no"}})
        .to_string()
    };
    for (subtype, expected) in [
        (
            "success",
            &[
                "initialize",
                "get_context_usage",
                "set_model",
                "get_context_usage",
            ][..],
        ),
        (
            "error",
            &["initialize", "get_context_usage", "set_model"][..],
        ),
    ] {
        let transport = FakeTransport::new(Vec::<String>::new())
            .wait_for_stdin(expect("initialize"))
            .then_stdout([init_answer()])
            .wait_for_stdin(expect("get_context_usage"))
            .wait_for_stdin(expect("set_model"))
            .then_stdout([answer(subtype)])
            .hold_open();
        let (session, _sink) = start(&transport).await;
        let changed =
            tokio::time::timeout(WAIT, session.change(SessionControl::Model("haiku".into())))
                .await
                .expect("answered in time");
        assert_eq!(changed.is_ok(), subtype == "success");
        assert_eq!(written(&transport), expected, "{subtype}");
        assert!(transport.stub_failures().is_empty());
        session.stop().await.unwrap();
    }
}

/// A `compact_boundary` line makes the driver ask, after the line in the log.
// verifies: PROTO-CLAUDE-CODE-STREAM-R72
#[tokio::test]
async fn the_cli_is_asked_after_each_compact_boundary() {
    let boundary = fixture("2.1.316/46-context-usage-during-compact")
        .stdout
        .into_iter()
        .find(|l| parse(l)["subtype"] == "compact_boundary")
        .expect("the recording compacted");
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize"))
        .then_stdout([init_answer()])
        .wait_for_stdin(expect("get_context_usage"))
        .then_stdout([boundary])
        .wait_for_stdin(expect("get_context_usage"))
        .then_stdout([r#"{"type":"system","subtype":"status","status":null}"#])
        .hold_open();
    let (session, sink) = start(&transport).await;
    // The stub writes this line only once the second request has come.
    sink.wait_for_line(WAIT, |l| l.line.contains(r#""status":null"#))
        .await;
    assert!(transport.stub_failures().is_empty());
    let host_after_boundary: Vec<String> = sink
        .lines()
        .into_iter()
        .skip_while(|l| !l.line.contains("compact_boundary"))
        .filter(|l| l.dir == Host)
        .map(|l| {
            parse(&l.line)["request"]["subtype"]
                .as_str()
                .unwrap_or("")
                .to_string()
        })
        .collect();
    assert_eq!(host_after_boundary, ["get_context_usage"]);
    session.stop().await.unwrap();
}

/// A refused request, or none answered at all, leaves the session working:
/// a message still goes out.
// verifies: PROTO-CLAUDE-CODE-STREAM-R73
#[tokio::test]
async fn a_refused_request_leaves_the_session_working() {
    let refusal = r#"{"type":"control_response","response":{"subtype":"error","request_id":"ersa-context-2","error":"unknown request"}}"#;
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize"))
        .then_stdout([init_answer()])
        .wait_for_stdin(expect("get_context_usage"))
        .then_stdout([refusal])
        .wait_for_stdin(|l| match parse(l)["type"].as_str() {
            Some("user") => Ok(()),
            _ => Err(format!("expected a message, got {l}")),
        })
        .then_stdout([r#"{"type":"system","subtype":"status","status":null}"#])
        .hold_open();
    let (session, sink) = start(&transport).await;
    sink.wait_for_line(WAIT, |l| l.line.contains("unknown request"))
        .await;
    session
        .send_user("hello")
        .await
        .expect("the session carries on");
    sink.wait_for_line(WAIT, |l| l.line.contains(r#""status":null"#))
        .await;
    assert!(transport.stub_failures().is_empty());
    // The refusal projected nothing but its passthrough.
    let refused = sink
        .lines()
        .into_iter()
        .find(|l| l.line.contains("unknown request"))
        .unwrap();
    assert!(
        project(&sink.lines())
            .into_iter()
            .filter(|(raw, _)| *raw == refused.index)
            .all(|(_, p)| matches!(p.event, Event::BackendPassthrough(_)))
    );
    session.stop().await.unwrap();
}

/// The request goes out within the handshake's time limit: a CLI that
/// answers `initialize` and then stops reading fails the start, saying so,
/// rather than holding it forever.
#[tokio::test]
async fn a_cli_that_stops_reading_after_initialize_fails_the_start_saying_so() {
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(expect("initialize"))
        .then_stdout([init_answer()])
        .hold_open()
        .wedge_stdin_after(1);
    let result = ClaudeAdapter::new()
        .with_handshake_timeout(Duration::from_millis(300))
        .with_stop_grace(Duration::from_millis(200))
        .start(
            Arc::new(transport),
            config(),
            Arc::new(RecordingSink::new()),
        )
        .await;
    let Err(ersa_adapter::AdapterError::Protocol(message)) = result else {
        panic!("expected the start to fail");
    };
    assert!(message.contains("stopped reading its input"), "{message}");
}
