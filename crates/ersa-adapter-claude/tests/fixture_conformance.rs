//! Replays the recorded Claude Code fixtures through the wire models.
//!
//! The fixtures live in `docs/briefs/protocol/fixtures/claude-code/<version>/`:
//! each `.jsonl` is verbatim CLI stdout, and each `.meta.json` records what was
//! written to stdin. These tests read them from the repository rather than
//! copying them, so a re-recording is tested without touching this file.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use ersa_adapter_claude::wire::*;
use serde_json::{Value, json};

/// `crates/ersa-adapter-claude` sits two levels below the workspace root.
fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits two levels below the workspace root")
        .join("docs/briefs/protocol/fixtures/claude-code")
}

/// Every file with the given suffix in every recorded version directory.
fn fixture_files(suffix: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for version in fs::read_dir(fixtures_root()).expect("fixtures directory exists") {
        let version = version.unwrap().path();
        if !version.is_dir() {
            continue;
        }
        for file in fs::read_dir(&version).unwrap() {
            let file = file.unwrap().path();
            if file.to_string_lossy().ends_with(suffix) {
                files.push(file);
            }
        }
    }
    files.sort();
    assert!(!files.is_empty(), "no {suffix} fixtures found");
    files
}

/// A fixture's `<version>/<file name>`, used to label failures.
fn label(file: &Path) -> String {
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    format!("{}/{}", name(file.parent().unwrap()), name(file))
}

/// Every non-empty stdout line of every fixture, with a `version/file:line` label.
fn fixture_lines() -> Vec<(String, String)> {
    let mut lines = Vec::new();
    for file in fixture_files(".jsonl") {
        let text = fs::read_to_string(&file).unwrap();
        let name = label(&file);
        for (n, line) in text.lines().enumerate() {
            if !line.trim().is_empty() {
                lines.push((format!("{name}:{}", n + 1), line.to_string()));
            }
        }
    }
    lines
}

/// The kinds this crate models, as `kind` labels. A fixture line of one of these
/// kinds must parse into its typed variant, never into `Unknown`/`Other`.
const MODELLED: &[&str] = &[
    "system/init",
    "system/status",
    "system/task_summary",
    "system/post_turn_summary",
    "system/thinking_tokens",
    "system/permission_denied",
    "assistant",
    "user",
    "stream_event",
    "result",
    "rate_limit_event",
    "control_request/can_use_tool",
    "control_response/success",
    "block/text",
    "block/tool_use",
    "block/thinking",
    "block/tool_result",
    "event/message_start",
    "event/content_block_start",
    "event/content_block_delta",
    "event/content_block_stop",
    "event/message_delta",
    "event/message_stop",
    "delta/text_delta",
];

/// Labels each typed or unknown piece of a parsed line, so the test can check
/// that every modelled kind was seen typed and report anything that was not.
#[derive(Default)]
struct Tally {
    typed: BTreeSet<String>,
    /// (where, kind) for every piece that landed in an `Unknown`/`Other` variant.
    unknown: Vec<(String, String)>,
}

impl Tally {
    fn unknown(&mut self, at: &str, prefix: &str, raw: &Value, tag: &str) {
        let kind = raw.get(tag).and_then(Value::as_str).unwrap_or("?");
        self.unknown
            .push((at.to_string(), format!("{prefix}{kind}")));
    }

    fn message(&mut self, at: &str, message: &CliMessage) {
        match message {
            CliMessage::System(system) => self.system(at, system),
            CliMessage::Assistant(m) => {
                self.typed.insert("assistant".into());
                self.blocks(at, &m.message.content);
            }
            CliMessage::User(m) => {
                self.typed.insert("user".into());
                if let MessageContent::Blocks(blocks) = &m.message.content {
                    self.blocks(at, blocks);
                }
            }
            CliMessage::StreamEvent(m) => {
                self.typed.insert("stream_event".into());
                self.event(at, &m.event);
            }
            CliMessage::Result(_) => {
                self.typed.insert("result".into());
            }
            CliMessage::RateLimitEvent(_) => {
                self.typed.insert("rate_limit_event".into());
            }
            CliMessage::ControlRequest(m) => match &m.request {
                CliRequest::CanUseTool(_) => {
                    self.typed.insert("control_request/can_use_tool".into());
                }
                CliRequest::Other(raw) => self.unknown(at, "control_request/", raw, "subtype"),
            },
            CliMessage::ControlResponse(m) => match &m.response {
                CliResponse::Success(_) => {
                    self.typed.insert("control_response/success".into());
                }
                CliResponse::Error(_) => {
                    self.typed.insert("control_response/error".into());
                }
                CliResponse::Other(raw) => self.unknown(at, "control_response/", raw, "subtype"),
            },
            // Modelled but unrecorded, so not in MODELLED.
            CliMessage::ControlCancelRequest(_) => {
                self.typed.insert("control_cancel_request".into());
            }
            CliMessage::KeepAlive { .. } => {
                self.typed.insert("keep_alive".into());
            }
            CliMessage::Unknown(raw) => self.unknown(at, "", raw, "type"),
        }
    }

    fn system(&mut self, at: &str, system: &SystemMessage) {
        let kind = match system {
            SystemMessage::Init(_) => "init",
            SystemMessage::Status(_) => "status",
            SystemMessage::TaskSummary(_) => "task_summary",
            SystemMessage::PostTurnSummary(_) => "post_turn_summary",
            SystemMessage::ThinkingTokens(_) => "thinking_tokens",
            SystemMessage::PermissionDenied(_) => "permission_denied",
            SystemMessage::Other(raw) => return self.unknown(at, "system/", raw, "subtype"),
        };
        self.typed.insert(format!("system/{kind}"));
    }

    fn blocks(&mut self, at: &str, blocks: &[ContentBlock]) {
        for block in blocks {
            let kind = match block {
                ContentBlock::Text { .. } => "text",
                ContentBlock::ToolUse { .. } => "tool_use",
                ContentBlock::Thinking { .. } => "thinking",
                ContentBlock::Image { .. } => "image",
                ContentBlock::ToolResult { content, .. } => {
                    if let Some(MessageContent::Blocks(inner)) = content {
                        self.blocks(at, inner);
                    }
                    "tool_result"
                }
                ContentBlock::Other(raw) => {
                    self.unknown(at, "block/", raw, "type");
                    continue;
                }
            };
            self.typed.insert(format!("block/{kind}"));
        }
    }

    fn event(&mut self, at: &str, event: &StreamEvent) {
        let kind = match event {
            StreamEvent::MessageStart { message } => {
                self.blocks(at, &message.content);
                "message_start"
            }
            StreamEvent::ContentBlockStart { content_block, .. } => {
                self.blocks(at, std::slice::from_ref(content_block));
                "content_block_start"
            }
            StreamEvent::ContentBlockDelta { delta, .. } => {
                match delta {
                    Delta::TextDelta { .. } => self.typed.insert("delta/text_delta".into()),
                    Delta::InputJsonDelta { .. } => {
                        self.typed.insert("delta/input_json_delta".into())
                    }
                    Delta::ThinkingDelta { .. } => self.typed.insert("delta/thinking_delta".into()),
                    Delta::SignatureDelta { .. } => {
                        self.typed.insert("delta/signature_delta".into())
                    }
                    Delta::Other(raw) => {
                        self.unknown(at, "delta/", raw, "type");
                        true
                    }
                };
                "content_block_delta"
            }
            StreamEvent::ContentBlockStop { .. } => "content_block_stop",
            StreamEvent::MessageDelta { .. } => "message_delta",
            StreamEvent::MessageStop => "message_stop",
            StreamEvent::Other(raw) => return self.unknown(at, "event/", raw, "type"),
        };
        self.typed.insert(format!("event/{kind}"));
    }
}

/// Makes two JSON values comparable after a parse and re-serialise: an absent
/// optional field comes back as `null` (or the other way round), and an integer
/// read into an `f64` comes back as a float. Neither loses information.
fn normalise(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), normalise(v)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(normalise).collect()),
        Value::Number(n) => json!(n.as_f64().unwrap()),
        other => other.clone(),
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R11
#[test]
fn every_fixture_line_parses_into_a_typed_variant() {
    let mut tally = Tally::default();
    for (at, line) in fixture_lines() {
        let message = CliMessage::from_line(&line)
            .unwrap_or_else(|e| panic!("{at} does not parse: {e}\n{line}"));
        tally.message(&at, &message);
    }

    // Report what landed in a passthrough variant. Anything listed in MODELLED
    // landing there means the model does not fit the recording.
    for (at, kind) in &tally.unknown {
        eprintln!("passthrough: {kind} at {at}");
    }
    let misparsed: Vec<_> = tally
        .unknown
        .iter()
        .filter(|(_, kind)| MODELLED.contains(&kind.as_str()))
        .collect();
    assert!(
        misparsed.is_empty(),
        "modelled kinds parsed as unknown: {misparsed:?}"
    );

    // And the fixtures do exercise every modelled kind, so none is untested.
    let unseen: Vec<_> = MODELLED
        .iter()
        .filter(|kind| !tally.typed.contains(**kind))
        .collect();
    assert!(
        unseen.is_empty(),
        "modelled kinds no fixture contains: {unseen:?}"
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R09
#[test]
fn parsing_keeps_every_field_of_every_fixture_line() {
    // Re-serialising a parsed line gives back the line: every field the models
    // do not name survives in an `extra` map or a passthrough variant.
    for (at, line) in fixture_lines() {
        let original: Value = serde_json::from_str(&line).unwrap();
        let message = CliMessage::from_line(&line).unwrap();
        let reserialised = serde_json::to_value(&message).unwrap();
        assert_eq!(
            normalise(&reserialised),
            normalise(&original),
            "{at} lost or changed data"
        );
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R09
#[test]
fn unknown_kinds_parse_as_passthrough_with_their_data() {
    let line = r#"{"type":"tool_progress","session_id":"s","novel":{"a":1}}"#;
    let CliMessage::Unknown(raw) = CliMessage::from_line(line).unwrap() else {
        panic!("an unmodelled type should be Unknown");
    };
    assert_eq!(raw, serde_json::from_str::<Value>(line).unwrap());

    let line = r#"{"type":"system","subtype":"compact_boundary","session_id":"s","uuid":"u","compact_metadata":{"trigger":"auto"}}"#;
    let CliMessage::System(SystemMessage::Other(raw)) = CliMessage::from_line(line).unwrap() else {
        panic!("an unmodelled system subtype should be System(Other)");
    };
    assert_eq!(raw["subtype"], "compact_boundary");
    assert_eq!(raw["compact_metadata"]["trigger"], "auto");

    let line = r#"{"type":"assistant","session_id":"s","uuid":"u","parent_tool_use_id":null,
        "message":{"id":"m","model":"x","role":"assistant","content":[{"type":"server_tool_use","id":"t"}]}}"#;
    let CliMessage::Assistant(m) = CliMessage::from_line(line).unwrap() else {
        panic!("assistant should parse");
    };
    assert_eq!(
        m.message.content,
        vec![ContentBlock::Other(
            json!({"type": "server_tool_use", "id": "t"})
        )]
    );

    let line = r#"{"type":"control_request","request_id":"r","request":{"subtype":"hook_callback","callback_id":"c"}}"#;
    let CliMessage::ControlRequest(m) = CliMessage::from_line(line).unwrap() else {
        panic!("control_request should parse");
    };
    assert_eq!(m.request_id, "r");
    assert!(matches!(m.request, CliRequest::Other(ref raw) if raw["callback_id"] == "c"));

    let line = r#"{"type":"stream_event","session_id":"s","uuid":"u","event":{"type":"ping"}}"#;
    let CliMessage::StreamEvent(m) = CliMessage::from_line(line).unwrap() else {
        panic!("stream_event should parse");
    };
    assert!(matches!(m.event, StreamEvent::Other(_)));

    // An unknown field on a modelled message is kept in `extra`.
    let line = r#"{"type":"system","subtype":"status","status":null,"session_id":"s","uuid":"u","new_field":7}"#;
    let CliMessage::System(SystemMessage::Status(status)) = CliMessage::from_line(line).unwrap()
    else {
        panic!("status should parse");
    };
    assert_eq!(status.extra["new_field"], 7);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R06
#[test]
fn capabilities_are_read_from_system_init() {
    let mut inits = 0;
    for (at, line) in fixture_lines() {
        let Ok(CliMessage::System(SystemMessage::Init(init))) = CliMessage::from_line(&line) else {
            continue;
        };
        inits += 1;
        let capabilities = init.capabilities();
        // The version directory a fixture sits in is the version that wrote it,
        // and the label starts with that directory.
        let version_dir = at.split('/').next().unwrap();
        assert_eq!(capabilities.claude_code_version, version_dir, "{at}");
        assert!(capabilities.has("interrupt_receipt_v1"), "{at}");
        assert!(capabilities.has("msg_lifecycle_v1"), "{at}");
        assert!(!capabilities.has("not_a_capability"), "{at}");
    }
    assert!(inits > 0, "no system/init line in the fixtures");
}

/// Every stdin line recorded in a `.meta.json`, with a `file#index` label.
fn recorded_stdin() -> Vec<(String, Value)> {
    let mut sent = Vec::new();
    for file in fixture_files(".meta.json") {
        let meta: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        let name = label(&file);
        for (i, entry) in meta["sent"].as_array().into_iter().flatten().enumerate() {
            let line = entry["line"].as_str().expect("sent entries hold a line");
            sent.push((format!("{name}#{i}"), serde_json::from_str(line).unwrap()));
        }
    }
    sent
}

#[test]
fn host_messages_serialise_to_the_recorded_stdin_lines() {
    let recorded = recorded_stdin();
    assert!(!recorded.is_empty(), "no recorded stdin lines");
    for (at, line) in recorded {
        // Build the message through the typed API, taking only the variable
        // parts (ids, text, tool input) from the recording.
        let built = match (line["type"].as_str(), line.pointer("/request/subtype")) {
            // Text blocks, and images as a message with attachments carries
            // them (fixture 2.1.316/07).
            (Some("user"), _) => HostMessage::user(
                line.pointer("/message/content")
                    .and_then(Value::as_array)
                    .unwrap()
                    .iter()
                    .map(|block| match block["type"].as_str() {
                        Some("image") => InputBlock::Image {
                            source: ImageSource {
                                kind: block["source"]["type"].as_str().unwrap().into(),
                                media_type: block["source"]["media_type"].as_str().unwrap().into(),
                                data: block["source"]["data"].as_str().unwrap().into(),
                            },
                        },
                        _ => InputBlock::Text {
                            text: block["text"].as_str().unwrap().into(),
                        },
                    })
                    .collect(),
            ),
            (Some("control_request"), Some(subtype)) if subtype == "initialize" => {
                // A trusted project's session attests trust in a folder
                // (fixture 2.1.316/03).
                let workspace_trust =
                    line.pointer("/request/workspaceTrust")
                        .map(|trust| WorkspaceTrust {
                            accepted: trust["accepted"].as_bool().unwrap(),
                            directory: trust["directory"].as_str().unwrap().into(),
                        });
                HostMessage::control_request(
                    line["request_id"].as_str().unwrap(),
                    HostRequest::Initialize(Box::new(InitializeRequest {
                        workspace_trust,
                        ..InitializeRequest::default()
                    })),
                )
            }
            // Stop: the driver's interrupt, recorded ending a turn (fixture 2.1.316/07).
            (Some("control_request"), Some(subtype)) if subtype == "interrupt" => {
                HostMessage::control_request(
                    line["request_id"].as_str().unwrap(),
                    HostRequest::Interrupt {
                        cancel_queued: None,
                    },
                )
            }
            (Some("control_request"), Some(subtype)) if subtype == "rename_session" => {
                let request = &line["request"];
                HostMessage::control_request(
                    line["request_id"].as_str().unwrap(),
                    HostRequest::RenameSession {
                        title: request["title"].as_str().unwrap().into(),
                        source: Some(RenameSource::Host),
                        session_id: request["session_id"].as_str().map(String::from),
                    },
                )
            }
            // Session controls, recorded between turns and during one
            // (fixtures 2.1.316/23 to 26).
            (Some("control_request"), Some(subtype)) if subtype == "set_model" => {
                HostMessage::control_request(
                    line["request_id"].as_str().unwrap(),
                    HostRequest::SetModel {
                        model: line
                            .pointer("/request/model")
                            .and_then(Value::as_str)
                            .map(String::from),
                    },
                )
            }
            (Some("control_request"), Some(subtype)) if subtype == "set_permission_mode" => {
                HostMessage::control_request(
                    line["request_id"].as_str().unwrap(),
                    HostRequest::SetPermissionMode {
                        mode: serde_json::from_value(line["request"]["mode"].clone())
                            .unwrap_or_else(|e| panic!("{at}: not a permission mode: {e}")),
                    },
                )
            }
            // No host message models `apply_flag_settings` yet: the ticket
            // that builds changing effort adds one. Until then the recorded
            // line must keep the shape the recordings settled.
            (Some("control_request"), Some(subtype)) if subtype == "apply_flag_settings" => {
                assert_eq!(
                    line,
                    json!({
                        "type": "control_request",
                        "request_id": line["request_id"],
                        "request": {
                            "subtype": "apply_flag_settings",
                            "settings": { "effortLevel": line["request"]["settings"]["effortLevel"] },
                        },
                    }),
                    "{at}"
                );
                continue;
            }
            (Some("control_response"), _) => HostMessage::permission_decision(
                line.pointer("/response/request_id")
                    .unwrap()
                    .as_str()
                    .unwrap(),
                // An allow or a deny, whichever was recorded (a question's
                // decline is a deny: fixture 2.1.316/09-question-deny).
                &serde_json::from_value::<PermissionDecision>(line["response"]["response"].clone())
                    .unwrap_or_else(|e| panic!("{at}: not a decision: {e}")),
            )
            .unwrap(),
            _ => panic!("{at}: no host message models this recorded line: {line}"),
        };
        let serialised: Value = serde_json::from_str(&built.to_line().unwrap()).unwrap();
        assert_eq!(serialised, line, "{at}");
    }
}

#[test]
fn host_messages_match_the_shapes_in_the_brief() {
    let line = |m: HostMessage| serde_json::from_str::<Value>(&m.to_line().unwrap()).unwrap();

    let deny = PermissionDecision::Deny {
        message: "not now".into(),
        interrupt: Some(false),
    };
    assert_eq!(
        line(HostMessage::permission_decision("r1", &deny).unwrap()),
        json!({"type": "control_response", "response": {"subtype": "success", "request_id": "r1",
            "response": {"behavior": "deny", "message": "not now", "interrupt": false}}})
    );

    let allow = PermissionDecision::Allow {
        updated_input: json!({"command": "ls"}),
        updated_permissions: Some(vec![PermissionUpdate::AddRules {
            rules: vec![PermissionRule {
                tool_name: "Bash".into(),
                rule_content: Some("ls *".into()),
            }],
            behavior: "allow".into(),
            destination: "session".into(),
        }]),
    };
    assert_eq!(
        line(HostMessage::permission_decision("r2", &allow).unwrap()),
        json!({"type": "control_response", "response": {"subtype": "success", "request_id": "r2",
            "response": {"behavior": "allow", "updatedInput": {"command": "ls"},
                "updatedPermissions": [{"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "ls *"}],
                    "behavior": "allow", "destination": "session"}]}}})
    );

    // The unrecorded requests, per the SDK types.
    let request = |r: HostRequest| line(HostMessage::control_request("q", r))["request"].clone();
    assert_eq!(
        request(HostRequest::Interrupt {
            cancel_queued: None
        }),
        json!({"subtype": "interrupt"})
    );
    assert_eq!(
        request(HostRequest::SetPermissionMode {
            mode: PermissionMode::AcceptEdits
        }),
        json!({"subtype": "set_permission_mode", "mode": "acceptEdits"})
    );
    assert_eq!(
        request(HostRequest::SetModel {
            model: Some("default".into())
        }),
        json!({"subtype": "set_model", "model": "default"})
    );
    assert_eq!(
        request(HostRequest::GetContextUsage {
            detail: Some(ContextUsageDetail::Summary)
        }),
        json!({"subtype": "get_context_usage", "detail": "summary"})
    );
    assert_eq!(
        request(HostRequest::GetSessionCost),
        json!({"subtype": "get_session_cost"})
    );
    assert_eq!(
        line(HostMessage::ControlCancelRequest {
            request_id: "q".into()
        }),
        json!({"type": "control_cancel_request", "request_id": "q"})
    );
    assert_eq!(
        request(HostRequest::Initialize(Box::new(InitializeRequest {
            append_system_prompt: Some("be brief".into()),
            sdk_mcp_servers: Some(vec!["ersa".into()]),
            ..Default::default()
        }))),
        json!({"subtype": "initialize", "hooks": {}, "sdkMcpServers": ["ersa"], "appendSystemPrompt": "be brief"})
    );
}

#[test]
fn the_initialize_response_and_permission_prompt_read_as_typed_payloads() {
    let mut saw_initialize = false;
    let mut saw_trust = false;
    let mut saw_prompt = false;
    let mut saw_question = false;
    for (at, line) in fixture_lines() {
        match CliMessage::from_line(&line).unwrap() {
            CliMessage::ControlResponse(CliControlResponse {
                response: CliResponse::Success(success),
            }) if success.request_id.starts_with("ersa-init") => {
                let init: InitializeResponse = success
                    .payload()
                    .unwrap_or_else(|e| panic!("{at}: initialize payload: {e}"));
                assert!(init.claude_code_version.is_some(), "{at}");
                assert!(!init.models.is_empty(), "{at}");
                if at.contains("workspace-trust") {
                    assert_eq!(init.workspace_trust_recorded, Some(true), "{at}");
                    saw_trust = true;
                } else {
                    assert_eq!(init.workspace_trust_recorded, None, "{at}");
                }
                saw_initialize = true;
            }
            CliMessage::ControlRequest(CliControlRequest {
                request: CliRequest::CanUseTool(prompt),
                ..
            }) if prompt.tool_name == "AskUserQuestion" => {
                // A question: its input is the questions, it needs the user,
                // and it offers no rules (PROTO-CLAUDE-CODE-STREAM, Questions).
                assert!(prompt.input["questions"].is_array(), "{at}");
                assert_eq!(prompt.requires_user_interaction, Some(true), "{at}");
                assert!(prompt.permission_suggestions.is_empty(), "{at}");
                assert!(prompt.description.is_none(), "{at}");
                saw_question = true;
            }
            CliMessage::ControlRequest(CliControlRequest {
                request: CliRequest::CanUseTool(prompt),
                ..
            }) => {
                assert_eq!(prompt.tool_name, "Bash", "{at}");
                assert!(prompt.blocked_path.is_some(), "{at}");
                assert!(matches!(
                    prompt.permission_suggestions.first(),
                    Some(PermissionUpdate::AddRules { .. })
                ));
                saw_prompt = true;
            }
            CliMessage::Assistant(m) if m.is_authentication_failure() => {
                assert_eq!(m.message.model, "<synthetic>", "{at}");
            }
            _ => {}
        }
    }
    assert!(saw_initialize && saw_prompt && saw_trust && saw_question);
}

// Unrecorded kinds, modelled from the SDK types: the shapes come from there
// until a fixture captures them.
// verifies: PROTO-CLAUDE-CODE-STREAM-R09
#[test]
fn keep_alive_and_control_cancel_request_parse_typed() {
    let line = r#"{"type":"keep_alive"}"#;
    assert!(matches!(
        CliMessage::from_line(line).unwrap(),
        CliMessage::KeepAlive { .. }
    ));

    let line =
        r#"{"type":"control_cancel_request","request_id":"1ddd13c0","reason":"interrupted"}"#;
    let CliMessage::ControlCancelRequest(cancel) = CliMessage::from_line(line).unwrap() else {
        panic!("control_cancel_request should parse typed");
    };
    assert_eq!(cancel.request_id, "1ddd13c0");
    assert_eq!(cancel.extra["reason"], "interrupted");

    // Both keep every field on the way back out.
    for line in [
        r#"{"type":"keep_alive","seq":3}"#,
        r#"{"type":"control_cancel_request","request_id":"r"}"#,
    ] {
        let reserialised = serde_json::to_value(CliMessage::from_line(line).unwrap()).unwrap();
        assert_eq!(reserialised, serde_json::from_str::<Value>(line).unwrap());
    }
}

/// Every entry of every session-record fixture (`<version>/record/*.jsonl`),
/// with a `version/record/file:line` label.
fn record_entries() -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for version in fs::read_dir(fixtures_root()).expect("fixtures directory exists") {
        let records = version.unwrap().path().join("record");
        if !records.is_dir() {
            continue;
        }
        let mut files: Vec<PathBuf> = fs::read_dir(&records)
            .unwrap()
            .map(|f| f.unwrap().path())
            .filter(|f| f.extension().is_some_and(|e| e == "jsonl"))
            .collect();
        files.sort();
        for file in files {
            let version = file.parent().unwrap().parent().unwrap();
            let name = format!(
                "{}/record/{}",
                version.file_name().unwrap().to_string_lossy(),
                file.file_name().unwrap().to_string_lossy()
            );
            let text = fs::read_to_string(&file).unwrap();
            for (n, line) in text.lines().enumerate() {
                if !line.trim().is_empty() {
                    entries.push((format!("{name}:{}", n + 1), line.to_string()));
                }
            }
        }
    }
    assert!(!entries.is_empty(), "no session-record fixtures found");
    entries
}

/// The record entry kinds this crate models.
const MODELLED_RECORD: &[&str] = &[
    "user",
    "assistant",
    "custom-title",
    "ai-title",
    "last-prompt",
    "system",
];

// verifies: PROTO-CLAUDE-CODE-STREAM-R11
#[test]
fn every_session_record_entry_parses_and_keeps_its_fields() {
    let mut typed = BTreeSet::new();
    for (at, line) in record_entries() {
        let entry =
            RecordEntry::from_line(&line).unwrap_or_else(|e| panic!("{at} does not parse: {e}"));
        let original: Value = serde_json::from_str(&line).unwrap();
        let kind = original["type"].as_str().unwrap_or("?").to_string();
        match &entry {
            RecordEntry::Other(_) => assert!(
                !MODELLED_RECORD.contains(&kind.as_str()),
                "{at}: a {kind} entry did not fit its model"
            ),
            RecordEntry::User(user) => {
                if let MessageContent::Blocks(blocks) = &user.message.content {
                    for block in blocks {
                        assert!(!matches!(block, ContentBlock::Other(_)), "{at}: {block:?}");
                    }
                }
                typed.insert(kind);
            }
            RecordEntry::Assistant(reply) => {
                for block in &reply.message.content {
                    assert!(!matches!(block, ContentBlock::Other(_)), "{at}: {block:?}");
                }
                typed.insert(kind);
            }
            _ => {
                typed.insert(kind);
            }
        }
        let reserialised = serde_json::to_value(&entry).unwrap();
        assert_eq!(
            normalise(&reserialised),
            normalise(&original),
            "{at} lost or changed data"
        );
    }
    // `ai-title` arises only in the interactive CLI and the IDE extensions,
    // which the recorder does not drive; the listing tests cover it.
    for kind in ["user", "assistant", "last-prompt", "custom-title"] {
        assert!(typed.contains(kind), "no record fixture has a {kind} entry");
    }
}

#[test]
fn record_tool_results_carry_the_permission_decision() {
    let decisions: Vec<RecordPermissionDecision> = record_entries()
        .into_iter()
        .filter_map(|(_, line)| match RecordEntry::from_line(&line).unwrap() {
            RecordEntry::User(user) => user.permission_decision,
            _ => None,
        })
        .collect();
    assert!(!decisions.is_empty());
    assert!(decisions.iter().all(|d| d.decision == "accept"));
    // One the user allowed when asked, one the permission mode allowed.
    assert!(
        decisions
            .iter()
            .any(|d| d.source.as_deref() == Some("user_temporary"))
    );
    assert!(
        decisions
            .iter()
            .any(|d| d.reason_type.as_deref() == Some("mode"))
    );
}

// ---------------------------------------------------------------------------
// Minimal mode (PROTO-CLAUDE-CODE-STREAM, Bare mode)

/// The flags the adapter adds for one session rather than for every one:
/// which session to resume or fork, the id it chooses, the fork's name, and
/// the model and effort when the user named them.
const PER_SESSION_FLAGS: &[&str] = &[
    "--resume",
    "--fork-session",
    "--session-id",
    "--name",
    "--model",
    "--effort",
];

/// The flag names in a command line, without their values and without the
/// per-session ones. Values vary with the session (its permission mode, the
/// setting sources trust allows), so two command lines are the same
/// invocation when they pass the same flags.
fn invocation_flags(args: &[String]) -> BTreeSet<String> {
    args.iter()
        .filter(|a| a.starts_with('-') && !PER_SESSION_FLAGS.contains(&a.as_str()))
        .cloned()
        .collect()
}

/// The first `system/init` of a stream fixture.
fn first_init(stream: &Path) -> Option<Value> {
    fs::read_to_string(stream)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|v| v["type"] == "system" && v["subtype"] == "init")
}

/// The stream fixtures of the newest tested version recorded with the
/// adapter's own invocation whose CLI started in its minimal mode, and the
/// ones left out because they were recorded with some other command line.
fn minimal_mode_at_newest_version() -> (Vec<String>, Vec<String>, usize) {
    // The newest version the adapter is tested against (PROTO-CLAUDE-CODE-STREAM-R10).
    let newest = fixtures_root().join(ersa_adapter_claude::MAX_TESTED_VERSION);
    let ours = invocation_flags(&ersa_adapter_claude::invocation_args(
        &ersa_adapter::SessionConfig::default(),
    ));
    let (mut minimal, mut not_counted, mut counted) = (Vec::new(), Vec::new(), 0);
    for meta in fs::read_dir(&newest).unwrap() {
        let meta = meta.unwrap().path();
        let name = meta.file_name().unwrap().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".meta.json").map(String::from) else {
            continue;
        };
        let recorded: Value = serde_json::from_str(&fs::read_to_string(&meta).unwrap()).unwrap();
        let args: Vec<String> = recorded["args"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        if invocation_flags(&args) != ours {
            not_counted.push(stem);
            continue;
        }
        counted += 1;
        if let Some(init) = first_init(&newest.join(format!("{stem}.jsonl")))
            && ersa_adapter_claude::started_in_minimal_mode(&init)
        {
            minimal.push(stem);
        }
    }
    minimal.sort();
    not_counted.sort();
    (minimal, not_counted, counted)
}

// A CLI version that starts in its minimal mode by default (no flag turns it
// off) shows up here as soon as its fixtures are recorded.
// verifies: PROTO-CLAUDE-CODE-STREAM-R54
#[test]
fn the_newest_version_does_not_start_in_minimal_mode_as_the_adapter_runs_it() {
    let (minimal, not_counted, counted) = minimal_mode_at_newest_version();
    assert!(
        counted > 0,
        "no fixture at the newest version was recorded as the adapter runs the CLI"
    );
    assert!(
        minimal.is_empty(),
        "Claude Code started in its minimal mode, which Ersa can't use, \
         in fixtures recorded with the adapter's own invocation: {minimal:?}"
    );
    // The fixture recorded with `--bare` is the evidence of what minimal mode
    // looks like. It is left out because it was not run as the adapter runs
    // the CLI; the check would catch it otherwise.
    assert!(
        not_counted.contains(&"37-bare-mode".to_string()),
        "{not_counted:?}"
    );
    let bare = first_init(&fixtures_root().join("2.1.316/37-bare-mode.jsonl")).unwrap();
    assert!(ersa_adapter_claude::started_in_minimal_mode(&bare));
    // A signed-out session (2.1.302/00-not-logged-in) is not minimal mode.
    let signed_out = first_init(&fixtures_root().join("2.1.302/00-not-logged-in.jsonl")).unwrap();
    assert!(!ersa_adapter_claude::started_in_minimal_mode(&signed_out));
    // Nor is a signed-out one with auto-memory off, which lacks two of the
    // three fields; its `--settings` leaves it out of the count above.
    assert!(
        not_counted.contains(&"38-signed-out-no-auto-memory".to_string()),
        "{not_counted:?}"
    );
    let no_memory =
        first_init(&fixtures_root().join("2.1.316/38-signed-out-no-auto-memory.jsonl")).unwrap();
    assert!(no_memory.get("scratchpad_path").is_none() && no_memory.get("memory_paths").is_none());
    assert!(!ersa_adapter_claude::started_in_minimal_mode(&no_memory));
}
