//! The projection from raw logs to Ersa events, over every recorded fixture
//! and a few hand-written logs for cases no fixture shows yet.
//!
//! Each fixture's projection is compared with a committed golden file under
//! `tests/golden/`. When a change to the projection is intended, rewrite them
//! with `ERSA_BLESS=1 cargo test -p ersa-adapter-claude --test projection` and
//! review the diff like code (PROTO-AGENT-EVENTS, Conformance).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use common::{events, fixture, fixtures, log, project};
use ersa_adapter::Direction::{Backend, Host};
use ersa_protocol::events::*;
use ersa_protocol::schema::validate_event;
use serde_json::{Value, json};

fn golden_path(fixture_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{fixture_name}.events.jsonl"))
}

/// One JSON object per event: the raw-line range it came from, then the event.
/// Written by hand so the range leads each line, where a reviewer looks first.
fn render(projection: &[(u64, ersa_adapter::Projected)]) -> String {
    let mut text = String::new();
    for (raw_to, projected) in projection {
        let event = serde_json::to_string(&projected.event).unwrap();
        let raw_from = projected.raw_from;
        text.push_str(&format!(
            r#"{{"raw_from":{raw_from},"raw_to":{raw_to},"event":{event}}}"#
        ));
        text.push('\n');
    }
    text
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R04
// verifies: PROTO-AGENT-EVENTS-R09
#[test]
fn every_fixture_projects_to_its_golden_file_and_validates() {
    let bless = std::env::var("ERSA_BLESS").is_ok_and(|v| v == "1");
    let mut mismatched = Vec::new();
    for fixture in fixtures() {
        let projection = project(&fixture.raw_log());
        for (_, projected) in &projection {
            let payload = serde_json::to_value(&projected.event).unwrap();
            validate_event(&payload).unwrap_or_else(|e| {
                panic!(
                    "{}: event failed the schema:\n{e}\n{payload:#}",
                    fixture.name
                )
            });
            assert!(projected.raw_from <= projection.iter().map(|(to, _)| *to).max().unwrap());
        }

        let rendered = render(&projection);
        let path = golden_path(&fixture.name);
        if bless {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &rendered).unwrap();
            continue;
        }
        // Git may check text files out with CRLF line ends on Windows.
        let golden = fs::read_to_string(&path)
            .unwrap_or_else(|_| {
                panic!("{} has no golden file; run with ERSA_BLESS=1", fixture.name)
            })
            .replace("\r\n", "\n");
        if golden != rendered {
            mismatched.push(fixture.name);
        }
    }
    assert!(
        mismatched.is_empty(),
        "projection differs from the golden file for {mismatched:?}; \
         if intended, rerun with ERSA_BLESS=1 and review the diff"
    );
}

// verifies: PROTO-AGENT-EVENTS-R03
#[test]
fn projecting_the_same_log_twice_gives_identical_events() {
    for fixture in fixtures() {
        let log = fixture.raw_log();
        assert_eq!(project(&log), project(&log), "{}", fixture.name);
    }
}

// verifies: PROTO-AGENT-EVENTS-R02
// verifies: PROTO-CLAUDE-CODE-STREAM-R09
#[test]
fn every_raw_line_yields_at_least_one_event() {
    for fixture in fixtures() {
        let log = fixture.raw_log();
        let projection = project(&log);
        for line in &log {
            assert!(
                projection.iter().any(|(to, _)| *to == line.index),
                "{}: raw line {} yielded no event",
                fixture.name,
                line.index
            );
        }
    }
}

// verifies: PROTO-AGENT-EVENTS-R07
#[test]
fn streamed_text_shares_a_block_id_and_ends_with_exactly_one_final() {
    let mut streamed_blocks = 0;
    for fixture in fixtures() {
        let mut blocks: BTreeMap<String, Vec<bool>> = BTreeMap::new();
        for event in events(&fixture.raw_log()) {
            if let Event::AssistantText(text) = event {
                blocks.entry(text.block_id).or_default().push(text.is_final);
            }
        }
        for (block_id, finals) in blocks {
            assert_eq!(
                finals.last(),
                Some(&true),
                "{}: {block_id} never ends",
                fixture.name
            );
            assert_eq!(
                finals.iter().filter(|f| **f).count(),
                1,
                "{}: {block_id}",
                fixture.name
            );
            if finals.len() > 1 {
                streamed_blocks += 1;
            }
        }
    }
    assert!(streamed_blocks > 0, "no fixture streamed a text block");

    // Fixture 02 streams "ready": a partial with the text so far, then the
    // final from the complete `assistant` line, covering the whole stream.
    let projection = project(&fixture("02-partial-messages").raw_log());
    let texts: Vec<_> = projection
        .iter()
        .filter_map(|(to, p)| match &p.event {
            Event::AssistantText(t) => Some((p.raw_from, *to, t.text.as_str(), t.is_final)),
            _ => None,
        })
        .collect();
    assert_eq!(texts, [(3, 4, "ready", false), (3, 5, "ready", true)]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R03
// verifies: PROTO-AGENT-EVENTS-R04
#[test]
fn can_use_tool_projects_to_one_permission_request_and_the_answer_to_a_decision() {
    let events = events(&fixture("11-stream-permission-prompt-tool-stdio").raw_log());
    let requests: Vec<&PermissionRequested> = events
        .iter()
        .filter_map(|e| match e {
            Event::PermissionRequested(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(requests.len(), 1);
    let request = requests[0];
    assert_eq!(request.request_id, "1ddd13c0-dd2c-4061-a8a8-86574059182e");
    assert_eq!(request.tool_use_id, "toolu_014GgKrCJ9rQgRzPv9jaW7rc");
    assert_eq!(request.tool_name, "Bash");
    assert_eq!(request.display_name.as_deref(), Some("Bash"));
    assert_eq!(
        request.description.as_deref(),
        Some("Write the word probe to probe.txt")
    );
    assert_eq!(request.input["command"], "echo probe > probe.txt");
    assert!(
        request
            .blocked_path
            .as_deref()
            .unwrap()
            .ends_with(r"ersa-rec-zww2Oe\probe.txt")
    );
    let kinds: Vec<_> = request.suggestions.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [SuggestionKind::AllowRule, SuggestionKind::AllowDirectory]
    );
    assert_eq!(
        request.suggestions[0].label,
        "Always allow Bash(echo probe *) in this project's local settings"
    );
    // The payload is the CLI's own object, for the driver to hand back.
    assert_eq!(
        request.suggestions[0].payload["rules"][0]["ruleContent"],
        "echo probe *"
    );
    assert_eq!(request.flags, PermissionFlags::default());

    // The shelf waits on the user, then the host's answer decides it.
    let position = |wanted: &dyn Fn(&Event) -> bool| events.iter().position(wanted).unwrap();
    let requested = position(&|e| matches!(e, Event::PermissionRequested(_)));
    assert_eq!(
        events[requested + 1],
        Event::SessionState(SessionState {
            state: LifecycleState::WaitingUser,
            reason: None
        })
    );
    let decided = position(&|e| matches!(e, Event::PermissionDecided(_)));
    assert!(decided > requested);
    assert_eq!(
        events[decided],
        Event::PermissionDecided(PermissionDecided {
            request_id: request.request_id.clone(),
            behavior: PermissionBehavior::Allow,
            decided_by: DecidedBy::User,
            message: None,
            applied_suggestions: None,
        })
    );
    assert_eq!(
        events[decided + 1],
        Event::SessionState(SessionState {
            state: LifecycleState::Running,
            reason: None
        })
    );
}

#[test]
fn the_host_user_message_starts_the_turn() {
    let events = events(&fixture("11-stream-permission-prompt-tool-stdio").raw_log());
    // Line 0 is the host's initialize, line 1 the CLI's answer, line 2 the
    // user message; `system/init` only comes after it.
    assert!(
        matches!(events[0], Event::BackendPassthrough(ref p) if p.kind == "control_request/initialize")
    );
    assert!(
        matches!(events[1], Event::BackendPassthrough(ref p) if p.kind == "control_response/success")
    );
    assert_eq!(
        events[2],
        Event::TurnStarted(TurnStarted {
            turn: 1,
            trigger: TurnTrigger::User
        })
    );
    assert!(
        matches!(events[4], Event::UserMessage(ref m) if m.text.starts_with("Use the Bash tool"))
    );
    assert!(
        matches!(events[5], Event::SessionStarted(ref s) if s.capabilities.contains(&"interrupt_receipt_v1".to_string()))
    );
}

#[test]
fn every_turn_event_falls_inside_a_turn() {
    for fixture in fixtures() {
        let mut open = false;
        for event in events(&fixture.raw_log()) {
            match event {
                Event::TurnStarted(_) => {
                    assert!(!open, "{}: a turn started inside a turn", fixture.name);
                    open = true;
                }
                Event::TurnCompleted(_) => {
                    assert!(open, "{}: turn completed with none open", fixture.name);
                    open = false;
                }
                Event::AssistantText(_)
                | Event::AssistantThinking(_)
                | Event::ToolRequested(_)
                | Event::ToolResult(_)
                | Event::PermissionRequested(_)
                | Event::PermissionDecided(_) => {
                    assert!(
                        open,
                        "{}: {} outside a turn",
                        fixture.name,
                        event.type_name()
                    );
                }
                _ => {}
            }
        }
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R08
// verifies: PROTO-AGENT-EVENTS-R05
#[test]
fn result_projects_to_one_turn_completed_and_one_usage_update() {
    let projection = project(&fixture("11-stream-permission-prompt-tool-stdio").raw_log());
    let result_line = 13;
    let from_result: Vec<&Event> = projection
        .iter()
        .filter(|(to, _)| *to == result_line)
        .map(|(_, p)| &p.event)
        .collect();
    assert_eq!(
        from_result,
        [
            &Event::TurnCompleted(TurnCompleted {
                turn: 1,
                outcome: TurnOutcome::Success,
                duration_ms: 10358,
                is_error: false,
                final_text: Some(
                    "The command succeeded. It exited cleanly with no output, and `probe.txt` now \
                     exists in `C:\\Users\\user\\AppData\\Local\\Temp\\ersa-rec-zww2Oe` containing \
                     the word \"probe\"."
                        .into()
                ),
            }),
            &Event::UsageUpdated(UsageUpdated {
                // The turn's last API call: 32 in, 136 cache writes, 40001
                // cache reads, 75 out. The top-level counts sum both calls.
                context_tokens: Some(40_244),
                context_window: Some(1_000_000),
                cost_usd_total: Some(0.8216802500000001),
                turns: Some(1),
                rate_limit: None,
            }),
            &Event::SessionState(SessionState { state: LifecycleState::Idle, reason: None }),
        ]
    );

    // Two turns: cost is the session's running total, as the CLI reports it.
    let costs: Vec<f64> = events(&fixture("07-stream-two-turns").raw_log())
        .into_iter()
        .filter_map(|e| match e {
            Event::UsageUpdated(u) => u.cost_usd_total,
            _ => None,
        })
        .collect();
    assert_eq!(costs, [0.77, 0.8826925]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R08
#[test]
fn rate_limit_event_projects_to_a_rate_limit_update() {
    let events = events(&fixture("11-stream-permission-prompt-tool-stdio").raw_log());
    let rate_limits: Vec<&RateLimit> = events
        .iter()
        .filter_map(|e| match e {
            Event::UsageUpdated(u) => u.rate_limit.as_ref(),
            _ => None,
        })
        .collect();
    assert_eq!(
        rate_limits,
        [&RateLimit {
            status: "allowed".into(),
            resets_at: Some("2026-11-04T20:00:00Z".into()),
            detail: Some("seven_day".into()),
        }]
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R05
// verifies: PROTO-AGENT-EVENTS-R06
#[test]
fn authentication_failure_is_a_session_error_not_transcript_text() {
    let events = events(&fixture("00-not-logged-in").raw_log());
    assert!(!events.iter().any(|e| matches!(e, Event::AssistantText(_))));
    let errors: Vec<&ErrorEvent> = events
        .iter()
        .filter_map(|e| match e {
            Event::Error(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(
        errors,
        [&ErrorEvent {
            code: ErrorCode::AuthenticationFailed,
            message: "Not logged in · Please run /login".into(),
            recoverable: true,
            action: Some(ErrorAction::LogIn),
        }]
    );
    // The turn still ends, as an error, without echoing the error as text.
    assert!(events.iter().any(|e| matches!(
        e,
        Event::TurnCompleted(TurnCompleted {
            outcome: TurnOutcome::Error,
            final_text: None,
            ..
        })
    )));
}

const INIT_REQUEST: &str = r#"{"type":"control_request","request_id":"ersa-init-1","request":{"subtype":"initialize","hooks":{}}}"#;

fn init_response(version: &str) -> String {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "ersa-init-1",
            "response": { "commands": [], "models": [], "claude_code_version": version }
        }
    })
    .to_string()
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R10
#[test]
fn a_cli_newer_than_tested_gets_a_warning_notice_and_the_session_continues() {
    let newer = init_response("2.2.0");
    let projected = events(&log(&[(Host, INIT_REQUEST), (Backend, &newer)]));
    assert_eq!(
        projected[1],
        Event::SessionNotice(SessionNotice {
            level: NoticeLevel::Warning,
            code: NoticeCode::BackendVersionUntested,
            message:
                "Claude Code 2.2.0 is newer than the newest version Ersa was tested with (2.1.302)."
                    .into(),
        })
    );

    for version in ["2.1.302", "2.1.0"] {
        let tested = init_response(version);
        let projected = events(&log(&[(Host, INIT_REQUEST), (Backend, &tested)]));
        assert!(
            matches!(projected[1], Event::BackendPassthrough(ref p) if p.kind == "control_response/success"),
            "{version}: {projected:?}"
        );
    }
}

// verifies: PROTO-AGENT-EVENTS-R10
// verifies: PROTO-AGENT-EVENTS-R02
#[test]
fn a_line_that_cannot_be_projected_is_a_recoverable_error_and_projection_continues() {
    let events = events(&log(&[
        (Backend, "not json {"),
        // A modelled type whose fields do not fit.
        (Backend, r#"{"type":"rate_limit_event","uuid":"u"}"#),
        (Backend, r#"{"type":"keep_alive"}"#),
    ]));
    let summary: Vec<String> = events
        .iter()
        .map(|e| match e {
            Event::Error(e) => format!("error {:?} recoverable={}", e.code, e.recoverable),
            Event::BackendPassthrough(p) => {
                format!("passthrough {} {}", p.kind, p.payload.is_string())
            }
            other => other.type_name().to_string(),
        })
        .collect();
    assert_eq!(
        summary,
        [
            "error Protocol recoverable=true",
            "passthrough unparsed true",
            "error Protocol recoverable=true",
            "passthrough rate_limit_event false",
            "passthrough keep_alive false",
        ]
    );
    assert_eq!(
        events[1],
        Event::BackendPassthrough(BackendPassthrough {
            backend: "claude-code".into(),
            kind: "unparsed".into(),
            payload: Value::String("not json {".into()),
        })
    );
}

#[test]
fn a_prompt_the_cli_withdraws_is_decided_by_the_backend() {
    let fixture_lines = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    let can_use_tool = &fixture_lines[4];
    let cancel =
        r#"{"type":"control_cancel_request","request_id":"1ddd13c0-dd2c-4061-a8a8-86574059182e"}"#;
    let events = events(&log(&[
        (Backend, can_use_tool),
        (Backend, cancel),
        (Backend, cancel),
    ]));
    let decided: Vec<&PermissionDecided> = events
        .iter()
        .filter_map(|e| match e {
            Event::PermissionDecided(d) => Some(d),
            _ => None,
        })
        .collect();
    assert_eq!(
        decided.len(),
        1,
        "a second cancel for the same id is only passed through"
    );
    assert_eq!(decided[0].behavior, PermissionBehavior::Deny);
    assert_eq!(decided[0].decided_by, DecidedBy::Backend);
}

#[test]
fn an_answer_that_applies_suggestions_names_their_indices() {
    let fixture_lines = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    let can_use_tool = &fixture_lines[4];
    let answer = json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "1ddd13c0-dd2c-4061-a8a8-86574059182e",
            "response": {
                "behavior": "allow",
                "updatedInput": { "command": "echo probe > probe.txt" },
                "updatedPermissions": [{
                    "type": "addDirectories",
                    "directories": ["C:\\Users\\user\\AppData\\Local\\Temp\\ersa-rec-zww2Oe"],
                    "destination": "session"
                }]
            }
        }
    })
    .to_string();
    let events = events(&log(&[(Backend, can_use_tool), (Host, &answer)]));
    let decided = events
        .iter()
        .find_map(|e| match e {
            Event::PermissionDecided(d) => Some(d),
            _ => None,
        })
        .expect("decided");
    assert_eq!(decided.applied_suggestions, Some(vec![1]));
}

// ---------------------------------------------------------------------------
// Drift, interleaving, and unfinished work

/// A short summary of each event, for asserting sequences.
fn summary(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            Event::TurnStarted(t) => format!("turn.started {} {:?}", t.turn, t.trigger),
            Event::TurnCompleted(t) => format!("turn.completed {} {:?}", t.turn, t.outcome),
            Event::SessionState(s) => format!("session.state {:?}", s.state),
            Event::AssistantText(t) => {
                format!("assistant.text {} {:?} {}", t.block_id, t.text, t.is_final)
            }
            Event::PermissionDecided(d) => format!("permission.decided {}", d.request_id),
            Event::PermissionRequested(r) => format!("permission.requested {}", r.request_id),
            Event::Error(e) => format!("error {:?}", e.code),
            Event::BackendPassthrough(p) => format!("passthrough {}", p.kind),
            other => other.type_name().to_string(),
        })
        .collect()
}

const USER_MESSAGE: &str =
    r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"hi"}]}}"#;

// verifies: PROTO-AGENT-EVENTS-R10
#[test]
fn a_result_whose_shape_drifted_still_closes_the_turn() {
    // `usage` is gone, so the typed model cannot read it.
    let drifted = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1200,"total_cost_usd":0.5,"session_id":"s","uuid":"u"}"#;
    let events = events(&log(&[
        (Host, USER_MESSAGE),
        (Backend, drifted),
        (Host, USER_MESSAGE),
    ]));
    assert_eq!(
        summary(&events),
        [
            "turn.started 1 User",
            "session.state Running",
            "user.message",
            "error Protocol",
            "turn.completed 1 Error",
            "usage.updated",
            "session.state Idle",
            "passthrough result/success",
            // The turn really closed: the next message starts turn 2.
            "turn.started 2 User",
            "session.state Running",
            "user.message",
        ]
    );
    let completed = events
        .iter()
        .find_map(|e| match e {
            Event::TurnCompleted(t) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(completed.duration_ms, 1200);
    assert_eq!(completed.final_text, None);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::UsageUpdated(u) if u.cost_usd_total == Some(0.5)))
    );
}

// verifies: PROTO-AGENT-EVENTS-R10
#[test]
fn an_init_whose_shape_drifted_still_starts_the_session() {
    // `permissionMode` and `apiKeySource` are gone.
    let drifted = r#"{"type":"system","subtype":"init","session_id":"s","uuid":"u","claude_code_version":"2.1.302","model":"m","cwd":"C:/w","capabilities":["interrupt_receipt_v1"]}"#;
    let events = events(&log(&[(Host, USER_MESSAGE), (Backend, drifted)]));
    assert_eq!(
        summary(&events)[3..],
        [
            "error Protocol",
            "session.started",
            "passthrough system/init"
        ]
    );
    assert_eq!(
        events[4],
        Event::SessionStarted(SessionStarted {
            backend: "claude-code".into(),
            backend_version: "2.1.302".into(),
            model: "m".into(),
            cwd: "C:/w".into(),
            capabilities: vec!["interrupt_receipt_v1".into()],
            backend_session_id: Some("s".into()),
        })
    );
}

// verifies: PROTO-AGENT-EVENTS-R04
#[test]
fn a_can_use_tool_whose_shape_drifted_still_reaches_the_shelf() {
    // Null suggestions, a structured reason, and no `tool_use_id`.
    let drifted = json!({
        "type": "control_request",
        "request_id": "r-9",
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "Bash",
            "input": { "command": "ls" },
            "permission_suggestions": null,
            "decision_reason": { "rule": "ask" },
            "default_to_no": true
        }
    })
    .to_string();
    let events = events(&log(&[(Backend, &drifted)]));
    let request = events
        .iter()
        .find_map(|e| match e {
            Event::PermissionRequested(r) => Some(r),
            _ => None,
        })
        .expect("a permission request");
    assert_eq!(request.request_id, "r-9");
    assert_eq!(request.tool_name, "Bash");
    assert_eq!(request.input["command"], "ls");
    assert!(request.suggestions.is_empty());
    assert!(request.flags.default_to_no);
    assert_eq!(
        request.reason,
        Some(PermissionReason {
            kind: "unspecified".into(),
            text: r#"{"rule":"ask"}"#.into(),
        })
    );
}

fn stream(parent: Option<&str>, event: Value) -> String {
    json!({ "type": "stream_event", "session_id": "s", "uuid": "u", "parent_tool_use_id": parent, "event": event })
        .to_string()
}

fn message_start(id: &str) -> Value {
    json!({ "type": "message_start", "message": { "id": id, "model": "m", "role": "assistant", "content": [] } })
}

fn text_delta(text: &str) -> Value {
    json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": text } })
}

// verifies: PROTO-AGENT-EVENTS-R07
#[test]
fn interleaved_main_and_subagent_streams_keep_their_own_blocks() {
    let events = events(&log(&[
        (Host, USER_MESSAGE),
        (Backend, &stream(None, message_start("main"))),
        (Backend, &stream(Some("toolu_sub"), message_start("sub"))),
        (Backend, &stream(None, text_delta("Hel"))),
        (Backend, &stream(Some("toolu_sub"), text_delta("Sub"))),
        (Backend, &stream(None, text_delta("lo"))),
    ]));
    let texts: Vec<String> = summary(&events)
        .into_iter()
        .filter(|s| s.starts_with("assistant.text"))
        .collect();
    assert_eq!(
        texts,
        [
            r#"assistant.text main/0 "Hel" false"#,
            r#"assistant.text sub/0 "Sub" false"#,
            r#"assistant.text main/0 "Hello" false"#,
        ]
    );
}

// verifies: PROTO-AGENT-EVENTS-R07
#[test]
fn blocks_still_streaming_when_the_turn_ends_are_finished() {
    let result = &fixture("11-stream-permission-prompt-tool-stdio").stdout[10];
    let projection = project(&log(&[
        (Host, USER_MESSAGE),
        (Backend, &stream(None, message_start("main"))),
        (Backend, &stream(Some("toolu_sub"), message_start("sub"))),
        (Backend, &stream(Some("toolu_sub"), text_delta("Sub"))),
        (Backend, &stream(None, text_delta("Interrupted mid"))),
        // An interrupt or API error: no complete `assistant` lines, then the end.
        (Backend, result),
    ]));
    let at_result: Vec<(u64, String)> = projection
        .iter()
        .filter(|(to, _)| *to == 5)
        .map(|(_, p)| {
            (
                p.raw_from,
                summary(std::slice::from_ref(&p.event)).remove(0),
            )
        })
        .collect();
    assert_eq!(
        at_result,
        [
            (3, r#"assistant.text sub/0 "Sub" true"#.to_string()),
            (
                4,
                r#"assistant.text main/0 "Interrupted mid" true"#.to_string()
            ),
            (5, "turn.completed 1 Success".to_string()),
            (5, "usage.updated".to_string()),
            (5, "session.state Idle".to_string()),
        ]
    );
}

#[test]
fn the_session_stays_waiting_while_any_prompt_is_pending() {
    let ask = &fixture("11-stream-permission-prompt-tool-stdio").stdout[4];
    let second = ask.replace("1ddd13c0-dd2c-4061-a8a8-86574059182e", "second");
    let answer = |id: &str| {
        json!({
            "type": "control_response",
            "response": { "subtype": "success", "request_id": id, "response": { "behavior": "deny", "message": "no" } }
        })
        .to_string()
    };
    let first_answer = answer("1ddd13c0-dd2c-4061-a8a8-86574059182e");
    let second_answer = answer("second");
    let events = events(&log(&[
        (Host, USER_MESSAGE),
        (Backend, ask),
        (Backend, &second),
        (Host, &first_answer),
        (Host, &second_answer),
    ]));
    assert_eq!(
        summary(&events)[3..],
        [
            "permission.requested 1ddd13c0-dd2c-4061-a8a8-86574059182e",
            "session.state WaitingUser",
            "permission.requested second",
            "session.state WaitingUser",
            // One answered, one still waiting: no `running` yet.
            "permission.decided 1ddd13c0-dd2c-4061-a8a8-86574059182e",
            "permission.decided second",
            "session.state Running",
        ]
    );
}

#[test]
fn a_message_queued_during_a_turn_gets_a_turn_of_its_own() {
    let out = fixture("07-stream-two-turns").stdout;
    let (init, result) = (&out[0], &out[5]);
    let events = events(&log(&[
        (Host, USER_MESSAGE),
        (Backend, init),
        // Sent while turn 1 runs: it joins turn 1 as a message...
        (Host, USER_MESSAGE),
        (Backend, result),
        // ...and the CLI's next `init` starts the turn that answers it.
        (Backend, init),
    ]));
    let turns: Vec<String> = summary(&events)
        .into_iter()
        .filter(|s| s.starts_with("turn."))
        .collect();
    assert_eq!(
        turns,
        [
            "turn.started 1 User",
            "turn.completed 1 Success",
            "turn.started 2 User"
        ]
    );
}
