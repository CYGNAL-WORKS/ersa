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

/// One JSON object per event: the raw line it came from and the message id
/// the session record shares, then the event. Written by hand so the line
/// leads, where a reviewer looks first.
fn render(projection: &[(u64, ersa_adapter::Projected)]) -> String {
    let mut text = String::new();
    for (raw, projected) in projection {
        let event = serde_json::to_string(&projected.event).unwrap();
        let message_id = match &projected.message_id {
            Some(id) => format!(r#""message_id":{},"#, serde_json::to_string(id).unwrap()),
            None => String::new(),
        };
        text.push_str(&format!(r#"{{"raw":{raw},{message_id}"event":{event}}}"#));
        text.push('\n');
    }
    text
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R11
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
            Event::AssistantText(t) => Some((*to, t.text.as_str(), t.is_final)),
            _ => None,
        })
        .collect();
    assert_eq!(texts, [(4, "ready", false), (5, "ready", true)]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R30
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

// verifies: PROTO-CORE-API-R25
#[test]
fn an_interrupted_turn_completes_as_interrupted() {
    // Recorded: an interrupt control request five seconds into a long answer.
    // The CLI ends the turn with an error result whose terminal_reason is
    // aborted_streaming.
    let completed: Vec<TurnCompleted> = events(&fixture("07-stream-interrupt").raw_log())
        .into_iter()
        .filter_map(|e| match e {
            Event::TurnCompleted(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].outcome, TurnOutcome::Interrupted);
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
                     exists in `C:/Users/user\\AppData\\Local\\Temp\\ersa-rec-zww2Oe` containing \
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
                turn_output_tokens: None,
                rate_limit: None,
            }),
            &Event::SessionState(SessionState {
                state: LifecycleState::Idle,
                reason: None
            }),
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
// verifies: PROTO-AGENT-EVENTS-R22
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

// A session Claude Code started in its minimal mode (`--bare`) fails its first
// message as if signed out, but signing in can't help: its `system/init` has
// none of `scratchpad_path`, `memory_paths` and `messaging_socket_path`
// (2.1.316/37-bare-mode).
// verifies: PROTO-CLAUDE-CODE-STREAM-R53
// verifies: PROTO-AGENT-EVENTS-R22
#[test]
fn a_session_started_in_minimal_mode_fails_with_its_own_reason_not_a_sign_in() {
    let events = events(&fixture("37-bare-mode").raw_log());
    assert!(!events.iter().any(|e| matches!(e, Event::AssistantText(_))));
    let errors: Vec<&ErrorEvent> = events
        .iter()
        .filter_map(|e| match e {
            Event::Error(e) => Some(e),
            _ => None,
        })
        .collect();
    let [error] = errors.as_slice() else {
        panic!("expected one error, got {errors:?}");
    };
    // The words are the adapter's; the fields are the requirement.
    assert_eq!(error.code, ErrorCode::BackendError);
    assert!(!error.recoverable);
    assert_eq!(error.action, None);
    assert!(!error.message.is_empty());
}

/// The one `error` a log projects to.
fn only_error(log: &[ersa_adapter::RawLine]) -> ErrorEvent {
    let errors: Vec<ErrorEvent> = events(log)
        .into_iter()
        .filter_map(|e| match e {
            Event::Error(e) => Some(e),
            _ => None,
        })
        .collect();
    let [error] = errors.as_slice() else {
        panic!("expected one error, got {errors:?}");
    };
    error.clone()
}

// A signed-out user with auto-memory off lacks `scratchpad_path` and
// `memory_paths` but keeps `messaging_socket_path`: signing in helps them.
// verifies: PROTO-CLAUDE-CODE-STREAM-R53
// verifies: PROTO-AGENT-EVENTS-R22
#[test]
fn a_signed_out_user_with_auto_memory_off_is_asked_to_sign_in() {
    let error = only_error(&fixture("38-signed-out-no-auto-memory").raw_log());
    assert_eq!(error.code, ErrorCode::AuthenticationFailed);
    assert_eq!(error.action, Some(ErrorAction::LogIn));
}

// Any one of the three fields says the CLI did not start in minimal mode: the
// bare fixture with one of them put back fails as a sign-in failure.
// verifies: PROTO-CLAUDE-CODE-STREAM-R53
// verifies: PROTO-AGENT-EVENTS-R22
#[test]
fn any_one_of_the_three_fields_means_not_minimal_mode() {
    for field in ["scratchpad_path", "memory_paths", "messaging_socket_path"] {
        let mut log = fixture("37-bare-mode").raw_log();
        let init = log
            .iter_mut()
            .find(|l| l.line.contains(r#""subtype":"init""#))
            .expect("the fixture has a system/init");
        let mut value: Value = serde_json::from_str(&init.line).unwrap();
        value[field] = json!("C:/somewhere");
        init.line = value.to_string();

        let error = only_error(&log);
        assert_eq!(error.code, ErrorCode::AuthenticationFailed, "{field}");
        assert_eq!(error.action, Some(ErrorAction::LogIn), "{field}");
    }
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
                "Claude Code 2.2.0 is newer than the newest version Ersa was tested with (2.1.316)."
                    .into(),
            pid: None,
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
                    "directories": ["C:/Users/user\\AppData\\Local\\Temp\\ersa-rec-zww2Oe"],
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
    // Finished in the order their streams started.
    let at_result: Vec<String> = projection
        .iter()
        .filter(|(to, _)| *to == 5)
        .map(|(_, p)| summary(std::slice::from_ref(&p.event)).remove(0))
        .collect();
    assert_eq!(
        at_result,
        [
            r#"assistant.text sub/0 "Sub" true"#,
            r#"assistant.text main/0 "Interrupted mid" true"#,
            "turn.completed 1 Success",
            "usage.updated",
            "session.state Idle",
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

// verifies: PROTO-AGENT-EVENTS-R04
#[test]
fn a_can_use_tool_missing_its_tool_name_and_input_still_reaches_the_shelf() {
    let defective = json!({
        "type": "control_request",
        "request_id": "r-10",
        "request": { "subtype": "can_use_tool", "input": "not an object" }
    })
    .to_string();
    let events = events(&log(&[(Host, USER_MESSAGE), (Backend, &defective)]));
    assert_eq!(
        summary(&events)[3..],
        [
            "error Protocol",
            "permission.requested r-10",
            "session.state WaitingUser",
            "passthrough control_request/can_use_tool",
        ]
    );
    let Event::PermissionRequested(request) = &events[4] else {
        panic!("expected the request");
    };
    assert_eq!(request.tool_name, "unknown");
    assert!(request.input.is_empty());
}

#[test]
fn prompts_left_pending_when_a_turn_ends_are_settled_by_the_backend() {
    let out = fixture("11-stream-permission-prompt-tool-stdio").stdout;
    let (ask, result) = (&out[4], &out[10]);
    let late_answer = json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": "1ddd13c0-dd2c-4061-a8a8-86574059182e",
            "response": { "behavior": "deny", "message": "too late" }
        }
    })
    .to_string();
    let events = events(&log(&[
        (Host, USER_MESSAGE),
        (Backend, ask),
        // The turn ends with the prompt unanswered and never withdrawn.
        (Backend, result),
        (Host, &late_answer),
    ]));
    assert_eq!(
        summary(&events)[3..],
        [
            "permission.requested 1ddd13c0-dd2c-4061-a8a8-86574059182e",
            "session.state WaitingUser",
            "permission.decided 1ddd13c0-dd2c-4061-a8a8-86574059182e",
            "turn.completed 1 Success",
            "usage.updated",
            "session.state Idle",
            // The late answer settles nothing and does not undo `idle`.
            "passthrough control_response/success",
        ]
    );
    assert!(matches!(
        &events[5],
        Event::PermissionDecided(d)
            if d.behavior == PermissionBehavior::Deny && d.decided_by == DecidedBy::Backend
    ));
}

/// The `turn_output_tokens` of each turn, in order: one list per turn.
fn turn_output_tokens(events: &[Event]) -> Vec<Vec<u64>> {
    let mut turns: Vec<Vec<u64>> = Vec::new();
    for event in events {
        match event {
            Event::TurnStarted(_) => turns.push(Vec::new()),
            Event::UsageUpdated(UsageUpdated {
                turn_output_tokens: Some(tokens),
                ..
            }) => turns
                .last_mut()
                .expect("a count outside a turn")
                .push(*tokens),
            _ => {}
        }
    }
    turns
}

// verifies: PROTO-AGENT-EVENTS-R17
#[test]
fn each_line_reporting_output_usage_projects_the_turns_output_tokens_so_far() {
    // Three turns with partial messages. Turns 2 and 3 each make two API
    // calls around a tool use, and each message's count arrives twice: an
    // early one on its `assistant` line (16), the final one on the later
    // `message_delta` (84).
    let fixture = fixture("00-session-tools");
    let events = events(&fixture.raw_log());
    assert_eq!(
        turn_output_tokens(&events),
        [vec![4, 4], vec![16, 84, 87, 87], vec![16, 84, 90, 90]]
    );

    // Each turn's last count is what the CLI's own `result` says the turn
    // generated, so no message was counted twice or missed.
    let results: Vec<u64> = fixture
        .stdout
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["type"] == "result")
        .map(|v| v["usage"]["output_tokens"].as_u64().unwrap())
        .collect();
    let last: Vec<u64> = turn_output_tokens(&events)
        .iter()
        .map(|t| *t.last().unwrap())
        .collect();
    assert_eq!(last, results);

    // A synthetic message (a failed login, a local command's output) made no
    // API call, so it gives no count.
    for name in ["00-not-logged-in", "03-local-command"] {
        let synthetic = common::events(&common::fixture(name).raw_log());
        assert_eq!(
            turn_output_tokens(&synthetic),
            [Vec::<u64>::new()],
            "{name}"
        );
    }

    // Only the count rides on these events, and the `result` line's usage
    // carries none: it belongs to the running turn.
    for event in &events {
        if let Event::UsageUpdated(usage) = event
            && usage.turn_output_tokens.is_some()
        {
            assert_eq!(
                usage,
                &UsageUpdated {
                    turn_output_tokens: usage.turn_output_tokens,
                    ..UsageUpdated::default()
                }
            );
        }
    }
}

// verifies: PROTO-AGENT-EVENTS-R17
#[test]
fn the_turns_output_tokens_never_decrease_and_start_again_each_turn() {
    let delta = |id: &str, tokens: u64| {
        json!({
            "type": "stream_event",
            "session_id": "s",
            "uuid": format!("d-{id}-{tokens}"),
            "api_message_id": id,
            "event": { "type": "message_delta", "delta": {}, "usage": { "output_tokens": tokens } }
        })
        .to_string()
    };
    let reply = |id: &str, tokens: u64| {
        json!({
            "type": "assistant",
            "session_id": "s",
            "uuid": format!("u-{id}-{tokens}"),
            "message": {
                "id": id, "model": "m", "role": "assistant",
                "content": [{ "type": "text", "text": "x" }],
                "usage": { "output_tokens": tokens }
            }
        })
        .to_string()
    };
    let result = json!({
        "type": "result", "subtype": "success", "is_error": false, "duration_ms": 1,
        "usage": { "output_tokens": 0 }
    })
    .to_string();
    let lines = [
        (Host, USER_MESSAGE.to_string()),
        (Backend, delta("m1", 50)),
        // An earlier count for m1 arriving late lowers nothing.
        (Backend, reply("m1", 10)),
        (Backend, reply("m2", 5)),
        (Backend, delta("m2", 7)),
        (Backend, result.clone()),
        // A stray count between turns opens no turn and counts toward none.
        (Backend, delta("m9", 99)),
        (Host, USER_MESSAGE.to_string()),
        // The same message id in a later turn is a new count.
        (Backend, delta("m1", 3)),
        // Absurd counts saturate rather than wrap.
        (Backend, delta("m3", u64::MAX)),
    ];
    let entries: Vec<(ersa_adapter::Direction, &str)> =
        lines.iter().map(|(d, l)| (*d, l.as_str())).collect();
    let events = events(&log(&entries));
    assert_eq!(
        turn_output_tokens(&events),
        [vec![50, 50, 55, 57], vec![3, u64::MAX]]
    );
    let started = events
        .iter()
        .filter(|e| matches!(e, Event::TurnStarted(_)))
        .count();
    assert_eq!(started, 2);
}

// ---------------------------------------------------------------------------
// Questions (PROTO-CLAUDE-CODE-STREAM, Questions), on the 2.1.316 recordings:
// the CLI asks three questions (single choice, multi-select, single choice)
// and the host answers, declines, or interrupts.

const ANSWERED_ID: &str = "0b943641-f720-4e34-be39-211ee791e85a";
const INTERRUPTED_ID: &str = "df763cc1-4136-4a49-873a-296881a62e00";
const DECLINED_ID: &str = "d7df0a3d-44a7-4570-a8fe-eb171fd6e8c8";

fn option(id: &str, label: &str, description: &str) -> QuestionOption {
    QuestionOption {
        id: id.into(),
        label: label.into(),
        description: Some(description.into()),
    }
}

fn question(
    id: &str,
    text: &str,
    header: &str,
    options: Vec<QuestionOption>,
    multi_select: bool,
) -> Question {
    Question {
        id: id.into(),
        text: text.into(),
        header: Some(header.into()),
        options: Some(options),
        multi_select,
        takes_text: true,
    }
}

fn the_question(events: &[Event]) -> &QuestionAsked {
    let asked: Vec<&QuestionAsked> = events
        .iter()
        .filter_map(|e| match e {
            Event::QuestionAsked(q) => Some(q),
            _ => None,
        })
        .collect();
    assert_eq!(asked.len(), 1, "one question.asked");
    asked[0]
}

fn the_answer(events: &[Event]) -> &QuestionAnswered {
    let answered: Vec<&QuestionAnswered> = events
        .iter()
        .filter_map(|e| match e {
            Event::QuestionAnswered(a) => Some(a),
            _ => None,
        })
        .collect();
    assert_eq!(answered.len(), 1, "one question.answered");
    answered[0]
}

fn no_permission_events(events: &[Event]) {
    assert!(
        !events.iter().any(|e| matches!(
            e,
            Event::PermissionRequested(_) | Event::PermissionDecided(_)
        )),
        "a question is not a permission request"
    );
}

/// The state the session moves to right after the first event `wanted` matches.
fn state_after(events: &[Event], wanted: &dyn Fn(&Event) -> bool) -> LifecycleState {
    let at = events.iter().position(wanted).expect("the event");
    match &events[at + 1] {
        Event::SessionState(s) => s.state,
        other => panic!("expected a session.state after it, got {other:?}"),
    }
}

// verifies: PROTO-AGENT-EVENTS-R18
// verifies: PROTO-CLAUDE-CODE-STREAM-R30
#[test]
fn the_question_tool_projects_to_one_question_and_no_permission_request() {
    let events = events(&fixture("2.1.316/07-question-answer").raw_log());
    no_permission_events(&events);
    assert_eq!(
        *the_question(&events),
        QuestionAsked {
            request_id: ANSWERED_ID.into(),
            questions: vec![
                question(
                    "0",
                    "Which color do you want?",
                    "Color",
                    vec![
                        option("0", "Red", "Warm color"),
                        option("1", "Blue", "Cool color")
                    ],
                    false,
                ),
                question(
                    "1",
                    "Which fruits do you want?",
                    "Fruits",
                    vec![
                        option("0", "Apple", "Crisp"),
                        option("1", "Pear", "Juicy"),
                        option("2", "Plum", "Tart"),
                    ],
                    true,
                ),
                question(
                    "2",
                    "Which name do you want?",
                    "Name",
                    vec![option("0", "Ann", "Short"), option("1", "Bob", "Simple")],
                    false,
                ),
            ],
        }
    );
    assert_eq!(
        state_after(&events, &|e| matches!(e, Event::QuestionAsked(_))),
        LifecycleState::WaitingUser
    );
}

// verifies: PROTO-AGENT-EVENTS-R19
#[test]
fn the_hosts_answer_projects_to_the_answers_by_question_and_option_id() {
    let events = events(&fixture("2.1.316/07-question-answer").raw_log());
    no_permission_events(&events);
    assert_eq!(
        *the_answer(&events),
        QuestionAnswered {
            request_id: ANSWERED_ID.into(),
            answers: vec![
                // "Blue": the second option.
                Answer {
                    id: "0".into(),
                    value: AnswerValue::One("1".into()),
                    own_words: false,
                },
                // "Apple, Pear": the first two options.
                Answer {
                    id: "1".into(),
                    value: AnswerValue::Many(vec!["0".into(), "1".into()]),
                    own_words: false,
                },
                // No option has this label: the user's own words.
                Answer {
                    id: "2".into(),
                    value: AnswerValue::One("Carol, from my own words".into()),
                    own_words: true,
                },
            ],
            declined: false,
            answered_by: AnsweredBy::User,
        }
    );
    assert_eq!(
        state_after(&events, &|e| matches!(e, Event::QuestionAnswered(_))),
        LifecycleState::Running
    );
}

// verifies: PROTO-AGENT-EVENTS-R19
#[test]
fn a_declined_question_projects_as_declined_with_no_answers() {
    let events = events(&fixture("2.1.316/09-question-deny").raw_log());
    no_permission_events(&events);
    assert_eq!(the_question(&events).request_id, DECLINED_ID);
    assert_eq!(
        *the_answer(&events),
        QuestionAnswered {
            request_id: DECLINED_ID.into(),
            answers: Vec::new(),
            declined: true,
            answered_by: AnsweredBy::User,
        }
    );
}

// verifies: PROTO-AGENT-EVENTS-R19
#[test]
fn a_question_the_cli_withdraws_after_an_interrupt_is_answered_by_the_backend() {
    let events = events(&fixture("2.1.316/08-question-interrupt").raw_log());
    no_permission_events(&events);
    assert_eq!(the_question(&events).request_id, INTERRUPTED_ID);
    assert_eq!(
        *the_answer(&events),
        QuestionAnswered {
            request_id: INTERRUPTED_ID.into(),
            answers: Vec::new(),
            declined: false,
            answered_by: AnsweredBy::Backend,
        }
    );
    let outcome = events
        .iter()
        .find_map(|e| match e {
            Event::TurnCompleted(t) => Some(t.outcome),
            _ => None,
        })
        .expect("the turn ends");
    assert_eq!(outcome, TurnOutcome::Interrupted);
}

// verifies: PROTO-AGENT-EVENTS-R19
#[test]
fn a_question_left_unanswered_when_the_turn_ends_is_answered_by_the_backend() {
    let out = fixture("2.1.316/07-question-answer").stdout;
    // The request, then the turn's `result` with nothing in between.
    let (ask, result) = (&out[3], &out[12]);
    let events = events(&log(&[
        (Host, USER_MESSAGE),
        (Backend, ask),
        (Backend, result),
    ]));
    no_permission_events(&events);
    assert_eq!(
        *the_answer(&events),
        QuestionAnswered {
            request_id: ANSWERED_ID.into(),
            answers: Vec::new(),
            declined: false,
            answered_by: AnsweredBy::Backend,
        }
    );
    let position = |wanted: &dyn Fn(&Event) -> bool| events.iter().position(wanted).unwrap();
    assert!(
        position(&|e| matches!(e, Event::QuestionAnswered(_)))
            < position(&|e| matches!(e, Event::TurnCompleted(_)))
    );
}

#[test]
fn questions_sharing_one_text_share_its_answer() {
    let request = json!({
        "type": "control_request",
        "request_id": "q",
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "AskUserQuestion",
            "input": { "questions": [
                { "question": "Which?", "options": [{ "label": "A" }, { "label": "B" }], "multiSelect": false },
                { "question": "Which?", "options": [{ "label": "B" }, { "label": "A" }], "multiSelect": true }
            ] }
        }
    })
    .to_string();
    let answer = json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": "q", "response": {
            "behavior": "allow",
            "updatedInput": { "questions": [], "answers": { "Which?": "A", "Nobody asked?": "x" } }
        } }
    })
    .to_string();
    let events = events(&log(&[
        (Host, USER_MESSAGE),
        (Backend, &request),
        (Host, &answer),
    ]));
    let answers: Vec<(&str, &AnswerValue)> = the_answer(&events)
        .answers
        .iter()
        .map(|a| (a.id.as_str(), &a.value))
        .collect();
    // Each question reads the value against its own options; the key that
    // names no question is left out.
    assert_eq!(
        answers,
        [
            ("0", &AnswerValue::One("0".into())),
            ("1", &AnswerValue::Many(vec!["1".into()])),
        ]
    );
}

// verifies: PROTO-AGENT-EVENTS-R10
#[test]
fn a_question_without_questions_still_reaches_the_user_with_an_error() {
    let request = json!({
        "type": "control_request",
        "request_id": "q",
        "request": { "subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": {} }
    })
    .to_string();
    let events = events(&log(&[(Host, USER_MESSAGE), (Backend, &request)]));
    assert_eq!(
        summary(&events),
        [
            "turn.started 1 User",
            "session.state Running",
            "user.message",
            "error Protocol",
            "question.asked",
            "session.state WaitingUser",
            "passthrough control_request/can_use_tool",
        ]
    );
    assert!(the_question(&events).questions.is_empty());
}

/// The `tool.requested` events a fixture's live stream projects, by tool name.
fn tool_requests(fixture_name: &str) -> Vec<ToolRequested> {
    events(&fixture(fixture_name).raw_log())
        .into_iter()
        .filter_map(|event| match event {
            Event::ToolRequested(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn the_request<'a>(requests: &'a [ToolRequested], name: &str) -> &'a ToolRequested {
    let mut named = requests.iter().filter(|r| r.name == name);
    let request = named.next().unwrap_or_else(|| panic!("no {name} call"));
    assert!(named.next().is_none(), "more than one {name} call");
    request
}

// Recorded on 2.1.316: each tool in Tool targets and edits that the CLI
// offers, and the tools its deferred ones load through.
// verifies: PROTO-CLAUDE-CODE-STREAM-R48
// verifies: PROTO-AGENT-EVENTS-R20
// verifies: PROTO-AGENT-EVENTS-R21
// verifies: PROTO-AGENT-EVENTS-R23
#[test]
fn recorded_tool_calls_carry_the_target_and_edits_the_table_gives() {
    const SCRATCH: &str = r"C:/Users/user\AppData\Local\Temp";
    let edit = |path: &str, old: Option<&str>, new: &str| FileEdit {
        path: path.into(),
        old_text: old.map(Into::into),
        new_text: new.into(),
        every_occurrence: false,
    };

    let requests = tool_requests("2.1.316/28-tool-edit");
    let notes = format!(r"{SCRATCH}\ersa-rec-tEzd1M\NOTES.md");
    let call = the_request(&requests, "Edit");
    assert_eq!(call.target.as_deref(), Some(notes.as_str()));
    assert_eq!(
        call.edits,
        Some(vec![edit(
            &notes,
            Some("Notes for the scratch repository."),
            "Notes, edited."
        )])
    );

    // A whole file written: no replaced text.
    let requests = tool_requests("2.1.316/29-tool-write");
    let hello = format!(r"{SCRATCH}\ersa-rec-QII59F\hello.txt");
    let call = the_request(&requests, "Write");
    assert_eq!(call.target.as_deref(), Some(hello.as_str()));
    assert_eq!(call.edits, Some(vec![edit(&hello, None, "Hello\nWorld\n")]));

    // An Edit with `replace_all` true: one edit, made everywhere.
    let requests = tool_requests("2.1.316/36-tool-edit-replace-all");
    let notes = format!(r"{SCRATCH}\ersa-rec-o32uKM\NOTES.md");
    let call = the_request(&requests, "Edit");
    assert_eq!(call.target.as_deref(), Some(notes.as_str()));
    assert_eq!(
        call.edits,
        Some(vec![FileEdit {
            every_occurrence: true,
            ..edit(&notes, Some("scratch"), "draft")
        }])
    );

    // NotebookEdit edits cells, not text: a target and no edits. ToolSearch
    // loads the deferred tool first, and targets its query.
    let requests = tool_requests("2.1.316/30-tool-notebook-edit");
    let notebook = format!(r"{SCRATCH}\ersa-rec-9CpFGX\nb.ipynb");
    let call = the_request(&requests, "NotebookEdit");
    assert_eq!(call.target.as_deref(), Some(notebook.as_str()));
    assert_eq!(call.edits, None);
    let call = the_request(&requests, "Read");
    assert_eq!(
        (call.target.as_deref(), &call.edits),
        (Some(notebook.as_str()), &None)
    );
    let call = the_request(&requests, "ToolSearch");
    assert_eq!(
        (call.target.as_deref(), &call.edits),
        (Some("select:NotebookEdit"), &None)
    );

    let requests = tool_requests("2.1.316/00-session-tools");
    let call = the_request(&requests, "Read");
    let readme = format!(r"{SCRATCH}\ersa-rec-m4YrBk\README.md");
    assert_eq!(
        (call.target.as_deref(), &call.edits),
        (Some(readme.as_str()), &None)
    );

    for (fixture_name, tool, target) in [
        ("2.1.316/31-tool-grep", "Grep", "scratch"),
        ("2.1.316/32-tool-glob", "Glob", "*.md"),
        (
            "2.1.316/33-tool-web-search",
            "WebSearch",
            "Tauri 2 release notes",
        ),
        (
            "2.1.302/05-bash-write-default",
            "Bash",
            "echo probe > probe.txt",
        ),
        (
            "2.1.316/33-tool-web-search",
            "ToolSearch",
            "select:WebSearch",
        ),
        (
            "2.1.316/35-tool-powershell",
            "PowerShell",
            "Get-ChildItem -Name",
        ),
        // The CLI lists `Task` in `system/init` but names the call `Agent`.
        ("2.1.316/34-tool-agent", "Agent", "Count files"),
        // The subagent's own call, inside the Agent call.
        ("2.1.316/34-tool-agent", "Glob", "*"),
    ] {
        let requests = tool_requests(fixture_name);
        let call = the_request(&requests, tool);
        assert_eq!(call.target.as_deref(), Some(target), "{fixture_name}");
        assert_eq!(call.edits, None, "{fixture_name}");
    }
}

// ---------------------------------------------------------------------------
// Messages written during a turn (PROTO-CLAUDE-CODE-STREAM, Sequencing)

/// The turn boundaries, states and user messages of a projection, each with
/// the raw line it was projected at.
fn turns_and_messages(lines: &[ersa_adapter::RawLine]) -> Vec<(u64, String)> {
    project(lines)
        .into_iter()
        .filter_map(|(at, projected)| {
            let what = match projected.event {
                Event::TurnStarted(t) => format!("turn.started {}", t.turn),
                Event::TurnCompleted(t) => format!("turn.completed {}", t.turn),
                Event::SessionState(s) => format!("session.state {:?}", s.state),
                Event::UserMessage(m) => format!("user.message {}", m.text),
                _ => return None,
            };
            Some((at, what))
        })
        .collect()
}

/// The raw line index of each `system/init` the CLI wrote.
fn inits(lines: &[ersa_adapter::RawLine]) -> Vec<u64> {
    lines
        .iter()
        .filter(|line| common::is_kind(line, "system/init"))
        .map(|line| line.index)
        .collect()
}

fn without_lines(sequence: Vec<(u64, String)>) -> Vec<String> {
    sequence.into_iter().map(|(_, what)| what).collect()
}

const ESSAY: &str = "user.message Write an essay of about 400 words on the history of lighthouses. Do not use any tools.";

// verifies: PROTO-CLAUDE-CODE-STREAM-R43
// verifies: PROTO-CLAUDE-CODE-STREAM-R44
#[test]
fn a_message_written_during_a_turn_starts_its_own_turn_at_that_turns_init() {
    let lines = fixture("22-message-during-turn").raw_log();
    let sequence = turns_and_messages(&lines);
    assert_eq!(
        without_lines(sequence.clone()),
        [
            "turn.started 1",
            "session.state Running",
            ESSAY,
            "turn.completed 1",
            // The second message waits in the CLI, so the session is still
            // working between the turns (R44).
            "session.state Running",
            "turn.started 2",
            "session.state Running",
            // Its message opens its own turn, not the one running when it
            // was written (R43).
            "user.message Now reply with the single word: second",
            "turn.completed 2",
            "session.state Idle",
        ]
    );
    // It is projected at the second turn's `system/init`.
    let second = sequence
        .iter()
        .find(|(_, what)| what.ends_with("second"))
        .unwrap();
    assert_eq!(second.0, inits(&lines)[1]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R43
// verifies: PROTO-CLAUDE-CODE-STREAM-R44
#[test]
fn messages_written_during_one_turn_start_the_next_one_in_the_order_written() {
    // The CLI runs both in one turn (fixture 50): one `system/init`, and the
    // record's prompt holds both texts.
    let lines = fixture("50-two-messages-during-turn").raw_log();
    let sequence = turns_and_messages(&lines);
    assert_eq!(
        without_lines(sequence.clone()),
        [
            "turn.started 1",
            "session.state Running",
            ESSAY,
            "turn.completed 1",
            "session.state Running",
            "turn.started 2",
            "session.state Running",
            "user.message Now reply with the single word: second",
            "user.message Now reply with the single word: third",
            "turn.completed 2",
            "session.state Idle",
        ]
    );
    let second_init = inits(&lines)[1];
    for (at, what) in &sequence {
        if what.ends_with("second") || what.ends_with("third") {
            assert_eq!(*at, second_init, "{what}");
        }
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R44
#[test]
fn an_interrupted_turn_with_a_message_waiting_leaves_the_session_working() {
    // Interrupting ends only the running turn; the waiting message then runs
    // (fixture 24).
    let sequence = without_lines(turns_and_messages(
        &fixture("24-interrupt-with-waiting").raw_log(),
    ));
    let completed = sequence
        .iter()
        .position(|what| what == "turn.completed 1")
        .unwrap();
    assert_eq!(
        sequence[completed + 1..],
        [
            "session.state Running",
            "turn.started 2",
            "session.state Running",
            "user.message Now reply with the single word: second",
            "turn.completed 2",
            "session.state Idle",
        ]
    );
}

fn user_text(text: &str) -> String {
    json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}})
        .to_string()
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R43
// verifies: PROTO-CLAUDE-CODE-STREAM-R44
#[test]
fn a_message_written_between_turns_while_one_waits_waits_too() {
    let out = fixture("07-stream-two-turns").stdout;
    let (init, result) = (&out[0], &out[5]);
    let (first, second, third) = (user_text("one"), user_text("two"), user_text("three"));
    let events = events(&log(&[
        (Host, &first),
        (Backend, init),
        (Host, &second),
        (Backend, result),
        // After the `result`, but before the CLI has started the waiting
        // message's turn: the CLI holds this one too.
        (Host, &third),
        (Backend, init),
        (Backend, result),
    ]));
    let sequence: Vec<String> = summary(&events)
        .into_iter()
        .zip(&events)
        .filter_map(|(what, event)| match event {
            Event::UserMessage(m) => Some(format!("user.message {}", m.text)),
            Event::TurnStarted(_) | Event::TurnCompleted(_) | Event::SessionState(_) => Some(what),
            _ => None,
        })
        .collect();
    assert_eq!(
        sequence,
        [
            "turn.started 1 User",
            "session.state Running",
            "user.message one",
            "turn.completed 1 Success",
            "session.state Running",
            "turn.started 2 User",
            "session.state Running",
            "user.message two",
            "user.message three",
            "turn.completed 2 Success",
            "session.state Idle",
        ]
    );
}

#[test]
fn a_stray_message_delta_between_turns_opens_no_turn() {
    let out = fixture("07-stream-two-turns").stdout;
    let (init, result) = (&out[0], &out[5]);
    let (first, second) = (user_text("one"), user_text("two"));
    let delta = stream(
        None,
        json!({"type": "message_delta", "delta": {}, "usage": {"output_tokens": 7}}),
    );
    let started = |events: &[Event]| {
        events
            .iter()
            .filter(|e| matches!(e, Event::TurnStarted(_)))
            .count()
    };
    let events = events(&log(&[
        (Host, &first),
        (Backend, init),
        (Host, &second),
        (Backend, result),
        // Between the `result` and the waiting message's `system/init`.
        (Backend, &delta),
        (Backend, init),
    ]));
    assert_eq!(started(&events), 2);
}
