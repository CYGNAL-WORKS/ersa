//! Conformance of the Rust event types with `protocol/schema/agent-events.schema.json`.
//!
//! One hand-written example of every event type is serialized from the Rust
//! types and validated against the schema file. The negative cases check that
//! the schema stays closed everywhere except the two payloads PROTO-AGENT-EVENTS
//! leaves open.

use std::collections::BTreeSet;

use ersa_protocol::events::*;
use ersa_protocol::schema::{AGENT_EVENTS_SCHEMA, validate_envelope, validate_event};
use serde_json::{Value, json};

/// Turns a `json!({...})` object literal into the `Map` that tool inputs use.
fn object(value: Value) -> serde_json::Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("expected a JSON object, got {other}"),
    }
}

/// One example of every event type, filling every optional field in at least
/// one place so the schema sees each field's shape.
fn examples() -> Vec<Event> {
    vec![
        Event::SessionStarted(SessionStarted {
            backend: "claude-code".into(),
            backend_version: "2.1.302".into(),
            model: "claude-opus-5-5".into(),
            cwd: "D:/work/ersa".into(),
            capabilities: vec!["partial_messages".into(), "interrupt".into()],
            backend_session_id: Some("4f1c2a".into()),
        }),
        Event::SessionState(SessionState {
            state: LifecycleState::WaitingUser,
            reason: Some("permission requested".into()),
        }),
        Event::SessionEnded(SessionEnded {
            reason: EndReason::BackendExited,
            exit_code: Some(1),
        }),
        Event::TurnStarted(TurnStarted {
            turn: 1,
            trigger: TurnTrigger::User,
        }),
        Event::TurnCompleted(TurnCompleted {
            turn: 1,
            outcome: TurnOutcome::Success,
            duration_ms: 4210,
            is_error: false,
            final_text: Some("Done.".into()),
        }),
        Event::UserMessage(UserMessage {
            text: "Rename the module".into(),
            origin: MessageOrigin::PeerSession,
            peer: Some(Peer {
                session_id: "s-2".into(),
                name: "planner".into(),
            }),
        }),
        Event::AssistantText(AssistantText {
            block_id: "b-1".into(),
            text: "Looking at the module now.".into(),
            is_final: true,
        }),
        Event::AssistantThinking(AssistantThinking {
            block_id: "b-0".into(),
            summary: Some("Weighing two approaches".into()),
            is_final: false,
        }),
        Event::ToolRequested(ToolRequested {
            tool_use_id: "t-1".into(),
            name: "Bash".into(),
            display_name: Some("Run command".into()),
            input: object(json!({ "command": "cargo test" })),
            parent_tool_use_id: Some("t-0".into()),
        }),
        Event::ToolResult(ToolResult {
            tool_use_id: "t-1".into(),
            content: ToolResultContent::Structured(json!([{ "kind": "text", "text": "ok" }])),
            is_error: false,
            duration_ms: Some(812),
        }),
        Event::ToolProgress(ToolProgress {
            tool_use_id: "t-1".into(),
            detail: Some("Compiling".into()),
        }),
        Event::PermissionRequested(PermissionRequested {
            request_id: "r-1".into(),
            tool_use_id: "t-2".into(),
            tool_name: "Write".into(),
            display_name: Some("Write file".into()),
            description: Some("Create src/lib.rs".into()),
            input: object(json!({ "file_path": "src/lib.rs", "content": "" })),
            blocked_path: Some("src/lib.rs".into()),
            suggestions: vec![PermissionSuggestion {
                kind: SuggestionKind::AllowRule,
                label: "Always allow Write in this project".into(),
                payload: json!({ "type": "addRules", "rules": [{ "toolName": "Write" }] }),
            }],
            flags: PermissionFlags {
                default_to_no: false,
                suppress_always_allow: false,
                requires_user_interaction: true,
                classifier_approvable: false,
            },
            reason: Some(PermissionReason {
                kind: "outside_workspace".into(),
                text: "The path is outside the workspace".into(),
            }),
        }),
        Event::PermissionDecided(PermissionDecided {
            request_id: "r-1".into(),
            behavior: PermissionBehavior::Allow,
            decided_by: DecidedBy::User,
            message: Some("Go ahead".into()),
            applied_suggestions: Some(vec![0]),
        }),
        Event::PermissionDeniedUnanswered(PermissionDeniedUnanswered {
            tool_use_id: "t-3".into(),
            tool_name: "Bash".into(),
            message: "Permission denied: no one could answer".into(),
        }),
        Event::QuestionAsked(QuestionAsked {
            request_id: "q-1".into(),
            questions: vec![Question {
                id: "which".into(),
                text: "Which database?".into(),
                options: Some(vec![
                    QuestionOption {
                        id: "sqlite".into(),
                        label: "SQLite".into(),
                    },
                    QuestionOption {
                        id: "pg".into(),
                        label: "Postgres".into(),
                    },
                ]),
                multi_select: true,
            }],
        }),
        Event::QuestionAnswered(QuestionAnswered {
            request_id: "q-1".into(),
            answers: vec![
                Answer {
                    id: "which".into(),
                    value: AnswerValue::Many(vec!["sqlite".into()]),
                },
                Answer {
                    id: "why".into(),
                    value: AnswerValue::One("It is embedded".into()),
                },
            ],
            answered_by: AnsweredBy::User,
        }),
        Event::UsageUpdated(UsageUpdated {
            context_tokens: Some(41_200),
            context_window: Some(200_000),
            cost_usd_total: Some(0.42),
            turns: Some(3),
            rate_limit: Some(RateLimit {
                status: "allowed_warning".into(),
                resets_at: Some("2026-10-31T18:00:00Z".into()),
                detail: Some("80% of the five-hour limit used".into()),
            }),
        }),
        Event::Error(ErrorEvent {
            code: ErrorCode::AuthenticationFailed,
            message: "Not logged in".into(),
            recoverable: true,
            action: Some(ErrorAction::LogIn),
        }),
        Event::BackendPassthrough(BackendPassthrough {
            backend: "claude-code".into(),
            kind: "system/hook_started".into(),
            payload: json!({ "type": "system", "subtype": "hook_started", "hook_name": "x" }),
        }),
    ]
}

/// The example of the given event type.
fn example(type_name: &str) -> Event {
    examples()
        .into_iter()
        .find(|e| e.type_name() == type_name)
        .unwrap_or_else(|| panic!("no example of {type_name}"))
}

fn envelope(seq: u64, event: Event) -> Envelope {
    Envelope {
        v: PROJECTION_VERSION,
        session_id: "s-1".into(),
        seq,
        ts: "2026-10-31T12:00:00.123Z".into(),
        source: Source {
            backend: "claude-code".into(),
            backend_version: "2.1.302".into(),
            raw_from: seq,
            raw_to: seq,
        },
        event,
    }
}

fn schema() -> Value {
    serde_json::from_str(AGENT_EVENTS_SCHEMA).expect("schema is JSON")
}

// verifies: PROTO-AGENT-EVENTS-R09
#[test]
fn every_event_type_has_an_example() {
    // The type names the schema lists, read from each `oneOf` reference.
    let in_schema: BTreeSet<String> = schema()["$defs"]["event"]["oneOf"]
        .as_array()
        .expect("event is a oneOf")
        .iter()
        .map(|r| {
            let reference = r["$ref"].as_str().expect("each branch is a $ref");
            reference.trim_start_matches("#/$defs/").to_string()
        })
        .collect();
    let in_examples: BTreeSet<String> = examples()
        .iter()
        .map(|e| e.type_name().to_string())
        .collect();

    assert_eq!(in_examples, in_schema);
}

// verifies: PROTO-AGENT-EVENTS-R09
#[test]
fn every_example_validates_as_a_payload_and_in_an_envelope() {
    for (seq, event) in examples().into_iter().enumerate() {
        let payload = serde_json::to_value(&event).expect("serialize payload");
        if let Err(e) = validate_event(&payload) {
            panic!(
                "{} payload failed the schema:\n{e}\n{payload:#}",
                event.type_name()
            );
        }

        let full = serde_json::to_value(envelope(seq as u64, event.clone())).expect("serialize");
        if let Err(e) = validate_envelope(&full) {
            panic!(
                "{} envelope failed the schema:\n{e}\n{full:#}",
                event.type_name()
            );
        }
    }
}

// verifies: PROTO-AGENT-EVENTS-R09
#[test]
fn minimal_examples_validate_with_every_optional_field_absent() {
    let minimal = [
        Event::SessionState(SessionState {
            state: LifecycleState::Idle,
            reason: None,
        }),
        Event::ToolResult(ToolResult {
            tool_use_id: "t-1".into(),
            content: ToolResultContent::Text("ok".into()),
            is_error: false,
            duration_ms: None,
        }),
        Event::UsageUpdated(UsageUpdated::default()),
        Event::BackendPassthrough(BackendPassthrough {
            backend: "claude-code".into(),
            kind: "unparsed".into(),
            payload: Value::String("not json {".into()),
        }),
    ];
    for event in minimal {
        let payload = serde_json::to_value(&event).expect("serialize");
        assert!(
            !payload.to_string().contains("null"),
            "absent optional fields are omitted, not null: {payload}"
        );
        validate_event(&payload).unwrap_or_else(|e| panic!("{e}\n{payload}"));
    }
}

#[test]
fn every_example_round_trips_through_json() {
    for event in examples() {
        let json = serde_json::to_string(&envelope(7, event.clone())).expect("serialize");
        let parsed: Envelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.event, event);
    }
}

#[test]
fn schema_version_matches_the_rust_constant() {
    let v = &schema()["$defs"]["envelope"]["properties"]["v"]["const"];
    assert_eq!(v, &json!(PROJECTION_VERSION));
}

#[test]
fn wire_names_are_family_dot_verb() {
    let payload = serde_json::to_value(example("assistant.text")).expect("serialize");
    assert_eq!(
        payload,
        json!({
            "type": "assistant.text",
            "block_id": "b-1",
            "text": "Looking at the module now.",
            "final": true
        })
    );
}

// ---------------------------------------------------------------------------
// Negative cases

/// Adds `"vendor_field": 1` to the object at `pointer` inside `value`.
fn with_extra_field(mut value: Value, pointer: &str) -> Value {
    value
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("no object at {pointer}"))
        .insert("vendor_field".into(), json!(1));
    value
}

// verifies: PROTO-AGENT-EVENTS-R08
#[test]
fn an_extra_field_on_any_payload_fails() {
    // Every payload, passthrough included: the passthrough's own fields are
    // closed; only the object inside its `payload` is open.
    for event in examples() {
        let payload = serde_json::to_value(&event).expect("serialize");
        let bad = with_extra_field(payload, "");
        assert!(
            validate_event(&bad).is_err(),
            "{} accepted an extra field",
            event.type_name()
        );
    }
}

// verifies: PROTO-AGENT-EVENTS-R08
#[test]
fn an_extra_field_in_a_nested_protocol_object_fails() {
    let cases = [
        ("tool.requested", "/source"),
        ("permission.requested", "/event/flags"),
        ("permission.requested", "/event/reason"),
        ("permission.requested", "/event/suggestions/0"),
        ("user.message", "/event/peer"),
        ("question.answered", "/event/answers/0"),
        ("question.asked", "/event/questions/0/options/0"),
        ("usage.updated", "/event/rate_limit"),
        ("session.started", ""),
    ];
    for (type_name, pointer) in cases {
        let full = serde_json::to_value(envelope(1, example(type_name))).expect("serialize");
        let bad = with_extra_field(full, pointer);
        assert!(
            validate_envelope(&bad).is_err(),
            "accepted an extra field at {pointer} of {type_name}"
        );
    }
}

// verifies: PROTO-AGENT-EVENTS-R08
#[test]
fn the_two_open_payloads_accept_backend_fields() {
    let suggestion = serde_json::to_value(example("permission.requested")).expect("serialize");
    let suggestion = with_extra_field(suggestion, "/suggestions/0/payload");
    validate_event(&suggestion).unwrap_or_else(|e| panic!("{e}"));

    let passthrough = serde_json::to_value(example("backend.passthrough")).expect("serialize");
    let passthrough = with_extra_field(passthrough, "/payload");
    validate_event(&passthrough).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn deserializing_rejects_unknown_fields_too() {
    let json = r#"{"type":"turn.started","turn":1,"trigger":"user","vendor_field":1}"#;
    assert!(serde_json::from_str::<Event>(json).is_err());
}

#[test]
fn malformed_envelopes_fail() {
    let good = serde_json::to_value(envelope(1, example("turn.started"))).expect("serialize");
    validate_envelope(&good).expect("the unmodified envelope is valid");

    let mut wrong_version = good.clone();
    wrong_version["v"] = json!(PROJECTION_VERSION + 1);
    assert!(validate_envelope(&wrong_version).is_err());

    let mut bad_ts = good.clone();
    bad_ts["ts"] = json!("yesterday");
    assert!(validate_envelope(&bad_ts).is_err());

    let mut unknown_type = good.clone();
    unknown_type["event"]["type"] = json!("turn.paused");
    assert!(validate_envelope(&unknown_type).is_err());

    let mut zero_turn = good;
    zero_turn["event"]["turn"] = json!(0);
    assert!(validate_envelope(&zero_turn).is_err());
}
