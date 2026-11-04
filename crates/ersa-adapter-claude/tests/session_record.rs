//! The CLI's session record: its projection, against the stream recorded for
//! the same run, and the list of a directory's sessions
//! (PROTO-CLAUDE-CODE-STREAM, Session record and Listing).
//!
//! Each `<version>/record/<stem>.jsonl` fixture is the record as it stood
//! after the run whose stream is `<version>/<stem>.jsonl`. Its projection is
//! compared with a golden file under `tests/golden/<version>/record/`; rewrite
//! them with `ERSA_BLESS=1 cargo test -p ersa-adapter-claude --test
//! session_record` and review the diff.

mod common;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use common::{fixtures, fixtures_root};
use ersa_adapter::{Direction, Projector};
use ersa_adapter_claude::ClaudeProjector;
use ersa_adapter_claude::record::{
    RecordCache, RecordEvent, SessionList, encoded_cwd, find_record, list_sessions, project_record,
    read_record, record_version,
};
use ersa_protocol::events::*;
use ersa_protocol::schema::validate_event;
use serde_json::{Value, json};

/// A record fixture: its `<version>/<stem>` name and its text.
struct RecordFixture {
    name: String,
    text: String,
}

fn record_fixtures() -> Vec<RecordFixture> {
    let mut found = Vec::new();
    for version in fs::read_dir(fixtures_root()).unwrap() {
        let version = version.unwrap().path();
        let Ok(files) = fs::read_dir(version.join("record")) else {
            continue;
        };
        for file in files {
            let file = file.unwrap().path();
            let name = file.file_name().unwrap().to_str().unwrap().to_string();
            let Some(stem) = name.strip_suffix(".jsonl") else {
                continue;
            };
            found.push(RecordFixture {
                name: format!("{}/{stem}", version.file_name().unwrap().to_string_lossy()),
                // Git may check text files out with CRLF line ends on Windows.
                text: fs::read_to_string(&file).unwrap().replace("\r\n", "\n"),
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(!found.is_empty(), "no session-record fixtures found");
    found
}

/// A record made of these entries, each ended by a newline as the CLI writes.
fn record(entries: &[Value]) -> String {
    entries.iter().map(|e| format!("{e}\n")).collect()
}

/// The events of a record, without their attribution.
fn events_of(entries: &[Value]) -> Vec<Event> {
    project_record(&record(entries))
        .into_iter()
        .map(|e| e.event)
        .collect()
}

/// Events only the live stream yields: the turn a message belongs to, the
/// session's activity, and the running turn's output tokens. The record has
/// messages, not turns.
fn live_only(event: &Event) -> bool {
    matches!(
        event,
        Event::TurnStarted(_)
            | Event::SessionState(_)
            | Event::UsageUpdated(UsageUpdated {
                turn_output_tokens: Some(_),
                ..
            })
    )
}

// verifies: PROTO-AGENT-EVENTS-R17
#[test]
fn events_read_from_a_session_record_carry_no_turn_output_tokens() {
    let records = record_fixtures();
    assert!(!records.is_empty());
    for record in records {
        for event in project_record(&record.text) {
            let payload = serde_json::to_value(&event.event).unwrap();
            assert!(
                payload.get("turn_output_tokens").is_none(),
                "{}: {payload}",
                record.name
            );
        }
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R15
// verifies: PROTO-CLAUDE-CODE-STREAM-R23
// verifies: PROTO-CLAUDE-CODE-STREAM-R20
#[test]
fn record_messages_project_to_the_same_events_as_the_stream_messages() {
    let records = record_fixtures();
    let mut compared = 0;
    let mut local_commands = 0;
    let mut kinds = BTreeSet::new();
    for stream in fixtures() {
        let Some(record) = records.iter().find(|r| r.name == stream.name) else {
            continue;
        };

        // The stream's events, by the uuid of the `user`/`assistant` line
        // that produced them.
        let mut live: Vec<(String, Vec<Event>)> = Vec::new();
        let mut projector = ClaudeProjector::new();
        for line in stream.raw_log() {
            let events: Vec<Event> = projector
                .project(&line)
                .into_iter()
                .map(|p| p.event)
                .filter(|e| !live_only(e))
                .collect();
            let value: Value = serde_json::from_str(&line.line).unwrap();
            let is_message = matches!(value["type"].as_str(), Some("user" | "assistant"));
            if line.dir == Direction::Backend && is_message {
                live.push((value["uuid"].as_str().unwrap().to_string(), events));
            }
        }
        assert!(
            !live.is_empty(),
            "{}: no messages on the stream",
            stream.name
        );

        let mut from_record: HashMap<String, Vec<Event>> = HashMap::new();
        for event in project_record(&record.text) {
            if let Some(id) = event.message_id {
                from_record.entry(id).or_default().push(event.event);
            }
        }
        // The kind each record entry has, by uuid.
        let record_kinds: HashMap<String, String> = record
            .text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter_map(|e| Some((e["uuid"].as_str()?.into(), e["type"].as_str()?.into())))
            .collect();
        let elided: HashSet<String> = record
            .text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|e| e["ersa_elided"] == true)
            .filter_map(|e| e["uuid"].as_str().map(String::from))
            .collect();
        for (uuid, events) in live {
            let kind = record_kinds
                .get(&uuid)
                .unwrap_or_else(|| panic!("{}: {uuid} is not in the record", stream.name));
            // An older fixture trimmed its local command's entry to its links.
            if elided.contains(&uuid) {
                continue;
            }
            let recorded = from_record
                .get(&uuid)
                .unwrap_or_else(|| panic!("{}: {uuid} is not in the record", stream.name));
            if kind == "system" {
                // A local command's output: a synthetic `assistant` message on
                // the stream, a `system/local_command` entry in the record.
                // R23: the same events, each block id being the entry's uuid
                // and the block's index, since the record lacks the synthetic
                // message's own id.
                let events: Vec<Event> = events
                    .into_iter()
                    .enumerate()
                    .map(|(index, event)| match event {
                        Event::AssistantText(text) => Event::AssistantText(AssistantText {
                            block_id: format!("{uuid}/{index}"),
                            text: text.text,
                            is_final: text.is_final,
                        }),
                        other => panic!("{}: {uuid}: unexpected {other:?}", stream.name),
                    })
                    .collect();
                assert!(!events.is_empty(), "{}: {uuid}", stream.name);
                assert_eq!(recorded, &events, "{}: {uuid}", stream.name);
                local_commands += 1;
                continue;
            }
            assert_eq!(recorded, &events, "{}: {uuid}", stream.name);
            for event in events {
                let kind = serde_json::to_value(&event).unwrap()["type"].clone();
                kinds.insert(kind.as_str().unwrap().to_string());
            }
            compared += 1;
        }
    }
    assert!(compared > 0, "no stream fixture has a record");
    assert!(local_commands > 0, "no local command's output was compared");
    // Every kind of message content the record carries was compared. The
    // model thinks only sometimes, so a thinking block is compared when a
    // recording has one.
    let mut expected = vec!["assistant.text", "tool.requested", "tool.result"];
    if records
        .iter()
        .any(|r| r.text.contains(r#""type":"thinking""#))
    {
        expected.push("assistant.thinking");
    }
    for kind in expected {
        assert!(kinds.contains(kind), "no {kind} was compared");
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R11
// verifies: PROTO-CLAUDE-CODE-STREAM-R15
#[test]
fn every_record_fixture_projects_to_its_golden_file_and_validates() {
    let bless = std::env::var("ERSA_BLESS").is_ok_and(|v| v == "1");
    let mut mismatched = Vec::new();
    for record in record_fixtures() {
        let projection = project_record(&record.text);
        let mut rendered = String::new();
        for event in &projection {
            let payload = serde_json::to_value(&event.event).unwrap();
            validate_event(&payload).unwrap_or_else(|e| {
                panic!(
                    "{}: event failed the schema:\n{e}\n{payload:#}",
                    record.name
                )
            });
            // A projection that could not read an entry fails the fixture.
            assert!(
                !matches!(event.event, Event::Error(_)),
                "{}: {:?}",
                record.name,
                event.event
            );
            rendered.push_str(
                &json!({ "message_id": event.message_id, "ts": event.ts, "backend_version": event.backend_version, "event": payload })
                    .to_string(),
            );
            rendered.push('\n');
        }

        let (version, stem) = record.name.split_once('/').unwrap();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(version)
            .join("record")
            .join(format!("{stem}.events.jsonl"));
        if bless {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &rendered).unwrap();
            continue;
        }
        let golden = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} has no golden file; run with ERSA_BLESS=1", record.name))
            .replace("\r\n", "\n");
        if golden != rendered {
            mismatched.push(record.name);
        }
    }
    assert!(
        mismatched.is_empty(),
        "projection differs from the golden file for {mismatched:?}; \
         if intended, rerun with ERSA_BLESS=1 and review the diff"
    );
}

fn prompt(uuid: &str, parent: Option<&str>, text: &str) -> Value {
    json!({"type": "user", "uuid": uuid, "parentUuid": parent, "isSidechain": false,
        "message": {"role": "user", "content": text}})
}

fn reply(uuid: &str, parent: &str, message_id: &str, text: &str) -> Value {
    json!({"type": "assistant", "uuid": uuid, "parentUuid": parent, "isSidechain": false,
        "message": {"id": message_id, "model": "m", "role": "assistant",
            "content": [{"type": "text", "text": text}]}})
}

fn texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::UserMessage(m) => Some(format!("user: {}", m.text)),
            Event::AssistantText(t) => Some(format!("assistant: {}", t.text)),
            _ => None,
        })
        .collect()
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R20
#[test]
fn only_the_branch_that_ends_the_record_is_projected() {
    // A prompt, a reply, a rewind to the first prompt with an edited second
    // prompt, and a subagent's entries written into the main record.
    let entries = [
        prompt("u1", None, "First"),
        reply("a1", "u1", "msg_1", "One"),
        prompt("u2", Some("a1"), "Abandoned"),
        reply("a2", "u2", "msg_2", "Abandoned reply"),
        json!({"type": "user", "uuid": "s1", "parentUuid": "a1", "isSidechain": true,
            "message": {"role": "user", "content": "Subagent task"}}),
        json!({"type": "assistant", "uuid": "s2", "parentUuid": "s1", "isSidechain": true,
            "message": {"id": "msg_s", "model": "m", "role": "assistant",
                "content": [{"type": "text", "text": "Subagent reply"}]}}),
        prompt("u3", Some("a1"), "Edited"),
        reply("a3", "u3", "msg_3", "Edited reply"),
        // Entries that are not messages can end the chain.
        json!({"type": "attachment", "uuid": "t1", "parentUuid": "a3", "isSidechain": false}),
        json!({"type": "custom-title", "customTitle": "Title", "sessionId": "s"}),
        // A subagent still working writes the latest leaf of all.
        json!({"type": "assistant", "uuid": "s3", "parentUuid": "s2", "isSidechain": true,
            "message": {"id": "msg_s2", "model": "m", "role": "assistant",
                "content": [{"type": "text", "text": "Subagent later"}]}}),
    ];
    assert_eq!(
        texts(&events_of(&entries)),
        [
            "user: First",
            "assistant: One",
            "user: Edited",
            "assistant: Edited reply"
        ]
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R20
#[test]
fn a_damaged_line_mid_record_loses_no_history_silently() {
    // The CLI crashed mid-write once; later entries chain through the entry
    // that line held.
    let text = format!(
        "{}\n{}\n{{\"type\":\"user\",\"uuid\":\"u2\",\"parentUuid\":\"a1\",\"mess\n{}\n{}\n",
        prompt("u1", None, "First"),
        reply("a1", "u1", "msg_1", "One"),
        reply("a2", "u2", "msg_2", "Two"),
        prompt("u3", Some("a2"), "Three"),
    );
    let events: Vec<Event> = project_record(&text).into_iter().map(|e| e.event).collect();
    assert_eq!(
        texts(&events),
        [
            "user: First",
            "assistant: One",
            "assistant: Two",
            "user: Three"
        ]
    );
    // The chain's break and the line itself are both reported.
    let errors = events
        .iter()
        .filter(|e| matches!(e, Event::Error(_)))
        .count();
    assert_eq!(errors, 2, "{events:?}");
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R20
#[test]
fn entries_that_form_no_chain_are_shown_in_file_order_with_an_error() {
    // Two entries naming each other: a cycle, no leaf. A subagent's and a
    // teammate's entries are still left out.
    let entries = [
        prompt("u1", Some("a1"), "Loop"),
        reply("a1", "u1", "msg_1", "Back"),
        json!({"type": "user", "uuid": "s1", "parentUuid": "a1", "isSidechain": true,
            "message": {"role": "user", "content": "Subagent task"}}),
        json!({"type": "user", "uuid": "t1", "parentUuid": "a1", "teamName": "crew",
            "message": {"role": "user", "content": "Teammate task"}}),
    ];
    let events = events_of(&entries);
    assert!(matches!(events[0], Event::Error(_)), "{events:?}");
    assert_eq!(texts(&events), ["user: Loop", "assistant: Back"]);
}

#[test]
fn an_entry_written_twice_is_projected_once() {
    let first = prompt("u1", None, "First");
    let entries = [
        first.clone(),
        reply("a1", "u1", "msg_1", "One"),
        first,
        prompt("u2", Some("a1"), "Second"),
    ];
    assert_eq!(
        texts(&events_of(&entries)),
        ["user: First", "assistant: One", "user: Second"]
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R15
// verifies: PROTO-CLAUDE-CODE-STREAM-R20
#[test]
fn parallel_tool_calls_all_stay_in_the_conversation() {
    // Two Read calls in one model message: 2.1.316 chains them one after the
    // other in the record (call, result, call, result), so no branch drops
    // one. Every call and result on the stream is in the record's projection.
    let name = "2.1.316/02-parallel-tools";
    let stream = fixtures()
        .into_iter()
        .find(|f| f.name == name)
        .expect("the fixture exists");
    let record = record_fixtures()
        .into_iter()
        .find(|r| r.name == name)
        .expect("its record exists");
    let tool_ids = |events: Vec<Event>| -> Vec<String> {
        events
            .into_iter()
            .filter_map(|e| match e {
                Event::ToolRequested(t) => Some(format!("requested {}", t.tool_use_id)),
                Event::ToolResult(t) => Some(format!("result {}", t.tool_use_id)),
                _ => None,
            })
            .collect()
    };
    let live = tool_ids(common::events(&stream.raw_log()));
    let recorded = tool_ids(
        project_record(&record.text)
            .into_iter()
            .map(|e| e.event)
            .collect(),
    );
    assert_eq!(
        live.iter().filter(|t| t.starts_with("requested")).count(),
        2
    );
    assert_eq!(recorded, live);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R20
#[test]
fn a_compaction_keeps_the_history_before_it() {
    let entries = [
        prompt("u1", None, "Before"),
        reply("a1", "u1", "msg_1", "Old"),
        json!({"type": "system", "subtype": "compact_boundary", "uuid": "c1",
            "parentUuid": null, "logicalParentUuid": "a1", "isSidechain": false}),
        json!({"type": "user", "uuid": "u2", "parentUuid": "c1", "isSidechain": false,
            "isCompactSummary": true, "message": {"role": "user", "content": "Summary"}}),
        prompt("u3", Some("u2"), "After"),
    ];
    let events = events_of(&entries);
    assert_eq!(
        texts(&events),
        [
            "user: Before",
            "assistant: Old",
            "user: Summary",
            "user: After"
        ]
    );
    // The summary is the CLI's, not the person's.
    assert!(matches!(
        &events[2],
        Event::UserMessage(UserMessage {
            origin: MessageOrigin::System,
            ..
        })
    ));
}

#[test]
fn prompts_are_user_messages_and_what_the_cli_wrote_is_system() {
    let entries = [
        json!({"type": "user", "uuid": "u1", "timestamp": "2026-11-01T10:00:00.000Z", "version": "2.1.316",
            "message": {"role": "user", "content": [{"type": "text", "text": "Hello"}]}}),
        json!({"type": "user", "uuid": "u2", "isMeta": true,
            "message": {"role": "user", "content": "<local-command-caveat>Caveat</local-command-caveat>"}}),
        json!({"type": "user", "uuid": "u3",
            "message": {"role": "user", "content": "<command-name>/rename</command-name>"}}),
        json!({"type": "user", "uuid": "u4",
            "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user]"}]}}),
        json!({"type": "user", "uuid": "u5", "origin": {"kind": "task-notification"},
            "message": {"role": "user", "content": "A task finished"}}),
        json!({"type": "custom-title", "customTitle": "T", "sessionId": "s"}),
        json!({"type": "file-history-snapshot", "messageId": "m", "snapshot": {}}),
        json!({"type": "attachment", "uuid": "a", "attachment": {"type": "date"}}),
    ];
    let events = project_record(&record(&entries));
    assert_eq!(
        events[0],
        RecordEvent {
            message_id: Some("u1".into()),
            ts: Some("2026-11-01T10:00:00.000Z".into()),
            backend_version: Some("2.1.316".into()),
            event: Event::UserMessage(UserMessage {
                text: "Hello".into(),
                origin: MessageOrigin::Human,
                peer: None,
            }),
        }
    );
    // Four messages the CLI wrote, and nothing from the other entries.
    assert_eq!(events.len(), 5);
    for event in &events[1..] {
        assert!(
            matches!(
                event.event,
                Event::UserMessage(UserMessage {
                    origin: MessageOrigin::System,
                    ..
                })
            ),
            "{event:?}"
        );
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R15
#[test]
fn an_unreadable_or_drifted_complete_entry_is_an_error_and_kept() {
    let text = "\n  \t\n{not json\n";
    let events = project_record(text);
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(matches!(
        events[0].event,
        Event::Error(ErrorEvent {
            code: ErrorCode::Protocol,
            ..
        })
    ));
    assert!(matches!(&events[1].event, Event::BackendPassthrough(p) if p.kind == "unparsed"));

    // An assistant entry without its message: the shape drifted.
    let drifted = json!({"type": "assistant", "uuid": "a1", "requestId": "r"});
    let events = project_record(&record(std::slice::from_ref(&drifted)));
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.message_id.as_deref() == Some("a1")));
    assert!(matches!(events[0].event, Event::Error(_)));
    assert!(matches!(&events[1].event, Event::BackendPassthrough(p) if p.payload == drifted));
}

#[test]
fn a_half_written_last_line_is_left_for_the_next_read() {
    let first = prompt("u1", None, "First");
    let whole = record(&[first.clone(), reply("a1", "u1", "msg_1", "One")]);
    // The CLI is in the middle of writing the reply.
    let torn = &whole[..whole.len() - 20];
    assert_eq!(
        project_record(torn),
        project_record(&record(std::slice::from_ref(&first))),
        "the torn line yielded something"
    );
    // A whole last entry with no newline (a crash just before it) counts.
    let unterminated = whole.trim_end();
    assert_eq!(project_record(unterminated), project_record(&whole));
}

#[test]
fn an_oversized_line_is_an_error_without_a_copy_of_it() {
    let huge = format!(
        "{{\"type\":\"user\",\"x\":\"{}\"}}\n",
        "y".repeat(17 * 1024 * 1024)
    );
    let events = project_record(&huge);
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0].event, Event::Error(_)));
    let Event::BackendPassthrough(p) = &events[1].event else {
        panic!("{:?}", events[1]);
    };
    assert!(p.payload.as_str().unwrap().len() <= 4096);
}

// ---------------------------------------------------------------------------
// Listing

const SESSION_A: &str = "0b4ac7de-1f6e-4a39-9d52-5f1e0c3a7b11";
const SESSION_B: &str = "1c5bd8ef-2a7f-4b4a-8e63-6a2f1d4b8c22";
const CWD: &str = r"C:\work\repo";

fn user(text: &str) -> String {
    json!({"type": "user", "uuid": "u", "message": {"role": "user", "content": text}}).to_string()
}

fn entry(kind: &str, key: &str, value: &str) -> String {
    json!({"type": kind, key: value, "sessionId": SESSION_A}).to_string()
}

/// Entries the listing must step over: about 1 KiB each.
fn filler(count: usize) -> Vec<String> {
    let body = "x".repeat(1000);
    (0..count)
        .map(|_| {
            json!({"type": "attachment", "attachment": {"type": "note", "body": body}}).to_string()
        })
        .collect()
}

/// A scratch Claude home with one record per `(session id, lines)`.
fn home_with(records: &[(&str, Vec<String>)]) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects").join(encoded_cwd(CWD));
    fs::create_dir_all(&dir).unwrap();
    for (id, lines) in records {
        fs::write(dir.join(format!("{id}.jsonl")), lines.join("\n") + "\n").unwrap();
    }
    home
}

fn titles(home: &Path) -> Vec<(String, Option<String>)> {
    list_sessions(home, CWD)
        .unwrap()
        .sessions
        .into_iter()
        .map(|s| (s.session_id, s.title))
        .collect()
}

fn title_of(lines: Vec<String>) -> Option<String> {
    let home = home_with(&[(SESSION_A, lines)]);
    let listed = list_sessions(home.path(), CWD).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert!(listed.unreadable.is_empty());
    listed.sessions[0].title.clone()
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R16
#[test]
fn a_session_is_titled_custom_then_ai_then_last_prompt_then_first_prompt() {
    let first = user("First prompt");
    let last = entry("last-prompt", "lastPrompt", "Last prompt");
    let ai = entry("ai-title", "aiTitle", "Generated title");
    let custom = entry("custom-title", "customTitle", "Given title");

    assert_eq!(
        title_of(vec![first.clone()]).as_deref(),
        Some("First prompt")
    );
    assert_eq!(
        title_of(vec![first.clone(), last.clone()]).as_deref(),
        Some("Last prompt")
    );
    // Without a `last-prompt` entry, the last prompt itself.
    assert_eq!(
        title_of(vec![first.clone(), user("Second prompt")]).as_deref(),
        Some("Second prompt")
    );
    assert_eq!(
        title_of(vec![first.clone(), ai.clone(), last.clone()]).as_deref(),
        Some("Generated title")
    );
    assert_eq!(
        title_of(vec![
            first.clone(),
            custom.clone(),
            ai.clone(),
            last.clone()
        ])
        .as_deref(),
        Some("Given title")
    );
    // The latest of each kind wins.
    assert_eq!(
        title_of(vec![
            custom.clone(),
            entry("custom-title", "customTitle", "Renamed"),
            first.clone(),
        ])
        .as_deref(),
        Some("Renamed")
    );
    // Tool results and text the CLI wrote are not prompts.
    let tool_result = json!({"type": "user", "uuid": "t", "message": {"role": "user",
        "content": [{"type": "tool_result", "tool_use_id": "x", "content": "ok"}]}})
    .to_string();
    let meta = json!({"type": "user", "uuid": "m", "isMeta": true,
        "message": {"role": "user", "content": "Caveat"}})
    .to_string();
    let command = user("<command-name>/rename</command-name>");
    let interrupted = user("[Request interrupted by user]");
    assert_eq!(
        title_of(vec![
            meta.clone(),
            first.clone(),
            tool_result.clone(),
            command.clone(),
            interrupted.clone()
        ])
        .as_deref(),
        Some("First prompt")
    );
    assert_eq!(
        title_of(vec![meta, tool_result, command, interrupted]),
        None
    );
}

#[test]
fn a_title_is_one_line_of_bounded_length() {
    assert_eq!(
        title_of(vec![user("Fix the\n\n  build   please")]).as_deref(),
        Some("Fix the build please")
    );
    let long = title_of(vec![user(&"word ".repeat(100))]).unwrap();
    assert_eq!(long.chars().count(), 200);
    assert!(long.ends_with('…'));
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R16
#[test]
fn listing_reads_only_the_head_and_the_tail_of_a_record() {
    // A record of several hundred KiB whose middle holds a custom title: the
    // listing never reads it, so the title comes from the ends.
    let mut lines = vec![user("First prompt")];
    lines.extend(filler(200));
    lines.push(entry("custom-title", "customTitle", "Lost in the middle"));
    lines.extend(filler(200));
    let home = home_with(&[(SESSION_A, lines.clone())]);
    assert_eq!(
        titles(home.path()),
        [(SESSION_A.to_string(), Some("First prompt".to_string()))]
    );

    // The same title near the end is read.
    lines.push(entry("ai-title", "aiTitle", "Near the end"));
    lines.extend(filler(10));
    let home = home_with(&[(SESSION_A, lines)]);
    assert_eq!(
        titles(home.path()),
        [(SESSION_A.to_string(), Some("Near the end".to_string()))]
    );
}

#[test]
fn a_line_cut_by_a_chunk_boundary_is_not_misread() {
    // A damaged line whose end is a whole title entry, placed so the tail
    // chunk begins exactly at that entry's `{`. Read from the boundary, the
    // fragment would parse as a title; the listing must drop the cut line.
    const CHUNK: usize = 64 * 1024;
    let fake = entry("custom-title", "customTitle", "Not a title");
    let last = user("Real last prompt");
    // What follows the fake entry fills the rest of the chunk exactly.
    let rest_len = CHUNK - fake.len();
    let padded_last = format!("{last}{}", " ".repeat(rest_len - last.len() - 1));
    let damaged = format!("{}{fake}", "y".repeat(100 * 1024));
    let lines = vec![user("First prompt"), damaged, padded_last];
    assert_eq!(title_of(lines).as_deref(), Some("Real last prompt"));

    // The head's cut line, likewise: a first prompt longer than a chunk is
    // not read from its start.
    let long_first = user(&"z".repeat(70 * 1024));
    assert_eq!(title_of(vec![long_first]), None);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R16
#[test]
fn the_recorded_session_lists_under_its_custom_title() {
    // The resume fixture's record, renamed with `/rename`, under its own id.
    let record = fixtures_root().join("2.1.316/record/01-session-resume.jsonl");
    let text = fs::read_to_string(record).unwrap();
    let id = text
        .lines()
        .find_map(|l| {
            serde_json::from_str::<Value>(l).ok()?["sessionId"]
                .as_str()
                .map(String::from)
        })
        .unwrap();
    let home = home_with(&[(id.as_str(), text.lines().map(String::from).collect())]);
    let listed = list_sessions(home.path(), CWD).unwrap().sessions;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, id);
    assert_eq!(listed[0].title.as_deref(), Some("Probe session"));

    // Without its title entry, the session lists under its last prompt.
    let untitled: Vec<String> = text
        .lines()
        .filter(|l| !l.contains("\"custom-title\""))
        .map(String::from)
        .collect();
    let home = home_with(&[(id.as_str(), untitled)]);
    assert_eq!(
        list_sessions(home.path(), CWD).unwrap().sessions[0]
            .title
            .as_deref(),
        Some("What word did you write to probe.txt? Reply with just that word.")
    );
}

#[test]
fn listing_skips_what_is_not_a_record_and_orders_newest_first() {
    let home = home_with(&[
        (SESSION_A, vec![user("Older")]),
        (SESSION_B, vec![user("Newer")]),
    ]);
    let dir = home.path().join("projects").join(encoded_cwd(CWD));
    // Not records: another file, a folder named like one, a name that is no id.
    fs::write(dir.join(format!("{SESSION_A}.desktop-released.json")), "{}").unwrap();
    fs::create_dir(dir.join(SESSION_B)).unwrap();
    fs::create_dir(dir.join("2d6ce9f0-3b8a-4c5b-9f74-7b3e2e5c9d33.jsonl")).unwrap();
    fs::write(dir.join("--resume.jsonl"), user("Hostile")).unwrap();
    let older = fs::File::options()
        .write(true)
        .open(dir.join(format!("{SESSION_A}.jsonl")))
        .unwrap();
    older
        .set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();

    let listed: SessionList = list_sessions(home.path(), CWD).unwrap();
    let ids: Vec<&str> = listed
        .sessions
        .iter()
        .map(|s| s.session_id.as_str())
        .collect();
    assert_eq!(ids, [SESSION_B, SESSION_A]);
    assert!(listed.unreadable.is_empty());

    // A directory with no records at all lists nothing.
    assert_eq!(
        list_sessions(home.path(), r"C:\elsewhere").unwrap(),
        SessionList::default()
    );
}

#[cfg(windows)]
#[test]
fn a_record_that_cannot_be_opened_is_reported_not_dropped() {
    use std::os::windows::fs::OpenOptionsExt;
    let home = home_with(&[
        (SESSION_A, vec![user("Readable")]),
        (SESSION_B, vec![user("Locked")]),
    ]);
    let locked = home
        .path()
        .join("projects")
        .join(encoded_cwd(CWD))
        .join(format!("{SESSION_B}.jsonl"));
    // Held open with no sharing, as another program might: opening it fails
    // with a sharing violation.
    let _held = fs::File::options()
        .read(true)
        .share_mode(0)
        .open(&locked)
        .unwrap();
    let listed = list_sessions(home.path(), CWD).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].session_id, SESSION_A);
    assert_eq!(listed.unreadable, [locked]);
}

/// A scratch Claude home holding `record` as session `id`'s record, under
/// the directory the CLI would use for `cwd`.
fn record_home(cwd: &str, id: &str, record: &str) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects").join(encoded_cwd(cwd));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{id}.jsonl")), record).unwrap();
    home
}

const SESSION: &str = "d34f0c1b-64ee-437e-a4e3-3487df29a506";

// verifies: PROTO-AGENT-EVENTS-R11
#[test]
fn a_session_record_read_twice_yields_identical_events() {
    for fixture in record_fixtures() {
        let home = record_home("C:/work", SESSION, &fixture.text);
        let read = || read_record(home.path(), "C:/work", SESSION).unwrap();
        let first = read();
        assert!(!first.is_empty(), "{}", fixture.name);
        assert_eq!(first, read(), "{}", fixture.name);
        assert_eq!(first, project_record(&fixture.text), "{}", fixture.name);
    }
}

#[test]
fn a_record_is_found_by_its_session_id_whatever_the_cwd_is_spelled() {
    let text = record_fixtures().remove(0).text;
    // The CLI spelled the directory `C:\work`; Core asks with `c:/work/`.
    let home = record_home(r"C:\work", SESSION, &text);
    let found = find_record(home.path(), "c:/work/", SESSION).unwrap();
    assert_eq!(
        found,
        Some(
            home.path()
                .join("projects")
                .join(encoded_cwd(r"C:\work"))
                .join(format!("{SESSION}.jsonl"))
        )
    );
    assert_eq!(
        read_record(home.path(), "c:/work/", SESSION).unwrap(),
        project_record(&text)
    );
}

#[test]
fn a_session_with_no_record_has_no_history_and_a_bad_id_is_refused() {
    let empty = tempfile::tempdir().unwrap();
    // No `projects` directory at all: the CLI never ran here.
    assert_eq!(read_record(empty.path(), "C:/work", SESSION).unwrap(), []);
    let home = record_home("C:/work", SESSION, "");
    let other = "00000000-0000-4000-8000-000000000000";
    assert_eq!(read_record(home.path(), "C:/work", other).unwrap(), []);
    // An id that is not a session id never becomes a path.
    let refused = read_record(home.path(), "C:/work", "../../secrets").unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
}

/// The merge key against the real CLI: every message event the projector
/// tags on a recorded stream carries a `uuid` the session record of the same
/// run gives a message with the same events, so Core can match them
/// (PROTO-AGENT-EVENTS-R12). The record may hold more: a resumed session's
/// record keeps the earlier run's messages too.
// verifies: PROTO-AGENT-EVENTS-R12
#[test]
fn live_message_ids_are_the_record_uuids_of_the_same_messages() {
    let records = record_fixtures();
    let mut matched = 0;
    for stream in fixtures() {
        let Some(record) = records.iter().find(|r| r.name == stream.name) else {
            continue;
        };
        let mut projector = ClaudeProjector::new();
        let live: BTreeSet<String> = stream
            .raw_log()
            .iter()
            .flat_map(|line| projector.project(line))
            .filter(|p| {
                matches!(
                    p.event,
                    Event::AssistantText(AssistantText { is_final: true, .. })
                        | Event::ToolRequested(_)
                        | Event::ToolResult(_)
                )
            })
            .filter_map(|p| p.message_id)
            .collect();
        let recorded: BTreeSet<String> = project_record(&record.text)
            .into_iter()
            .filter(|e| {
                matches!(
                    e.event,
                    Event::AssistantText(_) | Event::ToolRequested(_) | Event::ToolResult(_)
                )
            })
            .filter_map(|e| e.message_id)
            .collect();
        // Entries the fixture trimmed (a local command's output) keep their
        // uuid but nothing to project.
        let elided: BTreeSet<String> = record
            .text
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|e| e["ersa_elided"] == true)
            .filter_map(|e| e["uuid"].as_str().map(String::from))
            .collect();
        assert!(
            !live.is_empty(),
            "{}: no message ids on the stream",
            stream.name
        );
        let unmatched: Vec<&String> = live
            .iter()
            .filter(|id| !recorded.contains(*id) && !elided.contains(*id))
            .collect();
        assert!(unmatched.is_empty(), "{}: {unmatched:?}", stream.name);
        matched += 1;
    }
    assert!(matched > 0, "no stream fixture has a record beside it");
}

#[test]
fn a_cached_record_is_reused_while_unchanged_and_read_again_once_it_grows() {
    let text = record_fixtures().remove(0).text;
    let home = record_home("C:/work", SESSION, &text);
    let cache = RecordCache::default();
    let first = cache.read(home.path(), "C:/work", SESSION).unwrap();
    assert_eq!(*first, project_record(&text));
    assert_eq!(cache.reads(), 1);
    // Unchanged: the same projection, shared, and the file not read again.
    let second = cache.read(home.path(), "C:/work", SESSION).unwrap();
    assert!(std::sync::Arc::ptr_eq(&first, &second));
    assert_eq!(cache.reads(), 1);

    // The CLI appends a turn taken elsewhere: the record is read again.
    let path = find_record(home.path(), "C:/work", SESSION)
        .unwrap()
        .unwrap();
    let grown = format!(
        "{text}{}
",
        json!({"type": "user", "uuid": "u-later", "parentUuid": null,
            "message": {"role": "user", "content": "later"}})
    );
    fs::write(&path, &grown).unwrap();
    let again = cache.read(home.path(), "C:/work", SESSION).unwrap();
    assert_eq!(*again, project_record(&grown));
    assert_eq!(cache.reads(), 2);

    // A session Ersa no longer shows is let go.
    cache.forget(SESSION);
    cache.read(home.path(), "C:/work", SESSION).unwrap();
    assert_eq!(cache.reads(), 3);
}

#[test]
fn an_entry_too_long_to_read_is_reported_unless_it_is_still_being_written() {
    let first = json!({"type": "user", "uuid": "u1", "parentUuid": null,
        "message": {"role": "user", "content": "Hello"}});
    let long = format!(
        "{{\"type\":\"user\",\"uuid\":\"u2\",\"message\":{{\"content\":\"{}\"}}}}",
        "x".repeat(16 * 1024 * 1024 + 10)
    );
    // Whole, with its line end: reported, and the pass carries on.
    let ended = format!("{first}\r\n{long}\r\n{first}\n");
    let events: Vec<Event> = project_record(&ended)
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert!(
        events.iter().any(|e| matches!(e, Event::Error(_))),
        "{:?}",
        events.iter().map(|e| e.type_name()).collect::<Vec<_>>()
    );
    // The last line, still being written: left for the next read, quietly.
    let writing = format!("{first}\n{long}");
    let events: Vec<Event> = project_record(&writing)
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(
        events.len(),
        1,
        "{:?}",
        events.iter().map(|e| e.type_name()).collect::<Vec<_>>()
    );
    assert!(matches!(events[0], Event::UserMessage(_)));
}

#[test]
fn a_records_version_changes_when_it_grows() {
    let text = record_fixtures().remove(0).text;
    let home = record_home("C:/work", SESSION, &text);
    let before = record_version(home.path(), "C:/work", SESSION).unwrap();
    assert!(before.is_some());
    assert_eq!(
        record_version(home.path(), "C:/work", SESSION).unwrap(),
        before
    );
    let path = find_record(home.path(), "C:/work", SESSION)
        .unwrap()
        .unwrap();
    fs::write(&path, format!("{text}\n")).unwrap();
    assert_ne!(
        record_version(home.path(), "C:/work", SESSION).unwrap(),
        before
    );
    let other = "00000000-0000-4000-8000-000000000000";
    assert_eq!(record_version(home.path(), "C:/work", other).unwrap(), None);
}

fn local_command(uuid: &str, parent: Option<&str>, content: Value) -> Value {
    serde_json::json!({
        "type": "system", "subtype": "local_command", "uuid": uuid,
        "parentUuid": parent, "isSidechain": false, "content": content,
    })
}

fn blocks(events: &[Event]) -> Vec<(&str, &str)> {
    events
        .iter()
        .map(|e| match e {
            Event::AssistantText(t) => (t.block_id.as_str(), t.text.as_str()),
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

// The echo of the command (no output tag, as older versions wrote beside the
// output) yields nothing; each output tag yields its text, kept exactly,
// numbered under the entry's uuid.
// verifies: PROTO-CLAUDE-CODE-STREAM-R23
#[test]
fn only_a_local_commands_output_is_projected() {
    let events = events_of(&[
        local_command("a", None, "/allow".into()),
        local_command(
            "b",
            Some("a"),
            "<local-command-stdout>  name  size
  a.txt  3 </local-command-stdout>
             <local-command-stderr>Unknown command: /allow</local-command-stderr>
"
            .into(),
        ),
    ]);
    assert_eq!(
        blocks(&events),
        [
            (
                "b/0",
                "  name  size
  a.txt  3 "
            ),
            ("b/1", "Unknown command: /allow"),
        ]
    );
}

// A tag that is not closed, a changed wrapper, or content that is not a string
// is a drifted entry: an error and the entry kept as it came.
// verifies: PROTO-CLAUDE-CODE-STREAM-R23
#[test]
fn a_drifted_local_command_entry_is_reported_and_kept() {
    for content in [
        Value::from("<local-command-stdout>never closed"),
        Value::from("<local-command-output>x</local-command-output>"),
        serde_json::json!([{"type": "text", "text": "x"}]),
    ] {
        let events = events_of(&[local_command("a", None, content.clone())]);
        assert!(
            matches!(
                events.as_slice(),
                [Event::Error(_), Event::BackendPassthrough(p)] if p.payload["content"] == content
            ),
            "{content}: {events:?}"
        );
    }
}
