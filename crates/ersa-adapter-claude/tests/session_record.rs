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

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use common::{fixtures, fixtures_root};
use ersa_adapter::{Direction, Projector};
use ersa_adapter_claude::ClaudeProjector;
use ersa_adapter_claude::record::{
    RecordEvent, RecordProjector, SessionSummary, encoded_cwd, list_sessions,
};
use ersa_protocol::events::*;
use ersa_protocol::schema::validate_event;
use serde_json::{Value, json};

/// A record fixture: its `<version>/<stem>` name and its lines.
struct RecordFixture {
    name: String,
    lines: Vec<String>,
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
            let Some(stem) = file
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .strip_suffix(".jsonl")
            else {
                continue;
            };
            found.push(RecordFixture {
                name: format!("{}/{stem}", version.file_name().unwrap().to_string_lossy()),
                lines: fs::read_to_string(&file)
                    .unwrap()
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(String::from)
                    .collect(),
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(!found.is_empty(), "no session-record fixtures found");
    found
}

fn project_record(lines: &[String]) -> Vec<RecordEvent> {
    let mut projector = RecordProjector::new();
    lines
        .iter()
        .flat_map(|line| projector.project(line))
        .collect()
}

/// Events only the live stream yields: the turn a message belongs to and the
/// session's activity. The record has messages, not turns.
fn live_only(event: &Event) -> bool {
    matches!(event, Event::TurnStarted(_) | Event::SessionState(_))
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R15
#[test]
fn record_messages_project_to_the_same_events_as_the_stream_messages() {
    let records = record_fixtures();
    let mut compared = 0;
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
        for event in project_record(&record.lines) {
            if let Some(id) = event.message_id {
                from_record.entry(id).or_default().push(event.event);
            }
        }
        for (uuid, events) in live {
            let recorded = from_record
                .get(&uuid)
                .unwrap_or_else(|| panic!("{}: {uuid} is not in the record", stream.name));
            assert_eq!(recorded, &events, "{}: {uuid}", stream.name);
            for event in events {
                kinds.insert(
                    serde_json::to_value(&event).unwrap()["type"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                );
            }
            compared += 1;
        }
    }
    assert!(compared > 0, "no stream fixture has a record");
    // Every kind of message content the record carries was compared.
    for kind in [
        "assistant.text",
        "assistant.thinking",
        "tool.requested",
        "tool.result",
    ] {
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
        let projection = project_record(&record.lines);
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
                &json!({ "message_id": event.message_id, "ts": event.ts, "event": payload })
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

#[test]
fn prompts_in_the_record_are_user_messages_and_other_entries_yield_nothing() {
    let mut projector = RecordProjector::new();
    let prompt = json!({"type": "user", "uuid": "u1", "timestamp": "2026-11-01T10:00:00.000Z",
        "message": {"role": "user", "content": [{"type": "text", "text": "Hello"}]}});
    let meta = json!({"type": "user", "uuid": "u2", "isMeta": true,
        "message": {"role": "user", "content": "Caveat: added by the CLI"}});
    let events = projector.project(&prompt.to_string());
    assert_eq!(
        events,
        [RecordEvent {
            message_id: Some("u1".into()),
            ts: Some("2026-11-01T10:00:00.000Z".into()),
            event: Event::UserMessage(UserMessage {
                text: "Hello".into(),
                origin: MessageOrigin::Human,
                peer: None,
            }),
        }]
    );
    let events = projector.project(&meta.to_string());
    assert!(matches!(
        &events[0].event,
        Event::UserMessage(UserMessage {
            origin: MessageOrigin::System,
            ..
        })
    ));
    for ignored in [
        json!({"type": "custom-title", "customTitle": "T", "sessionId": "s"}),
        json!({"type": "file-history-snapshot", "messageId": "m", "snapshot": {}}),
        json!({"type": "attachment", "uuid": "a", "attachment": {"type": "date"}}),
    ] {
        assert_eq!(projector.project(&ignored.to_string()), [], "{ignored}");
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R15
#[test]
fn an_unreadable_or_drifted_message_entry_is_an_error_and_kept() {
    let mut projector = RecordProjector::new();
    let events = projector.project("{not json");
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
    let events = projector.project(&drifted.to_string());
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|e| e.message_id.as_deref() == Some("a1")));
    assert!(matches!(events[0].event, Event::Error(_)));
    assert!(matches!(&events[1].event, Event::BackendPassthrough(p) if p.payload == drifted));
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
        .into_iter()
        .map(|s| (s.session_id, s.title))
        .collect()
}

fn title_of(lines: Vec<String>) -> Option<String> {
    let home = home_with(&[(SESSION_A, lines)]);
    let listed = list_sessions(home.path(), CWD).unwrap();
    assert_eq!(listed.len(), 1);
    listed[0].title.clone()
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
    // Tool results and text the CLI added are not prompts.
    let tool_result = json!({"type": "user", "uuid": "t", "message": {"role": "user",
        "content": [{"type": "tool_result", "tool_use_id": "x", "content": "ok"}]}})
    .to_string();
    let meta = json!({"type": "user", "uuid": "m", "isMeta": true,
        "message": {"role": "user", "content": "Caveat"}})
    .to_string();
    assert_eq!(
        title_of(vec![meta.clone(), first.clone(), tool_result.clone()]).as_deref(),
        Some("First prompt")
    );
    assert_eq!(title_of(vec![meta, tool_result]), None);
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
    // A prompt longer than a chunk, ending in text shaped like a title entry:
    // the head cuts it at its end and the tail at its start, so neither reads
    // it, and nothing inside it is taken for an entry.
    let fake = format!(
        "
{}
",
        entry("custom-title", "customTitle", "Not a title")
    );
    let long_prompt = format!("{}{fake}", "y".repeat(80 * 1024));
    let lines = vec![user("First prompt"), user(&long_prompt)];
    assert_eq!(title_of(lines).as_deref(), Some("First prompt"));
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R16
#[test]
fn the_recorded_session_lists_under_its_last_prompt() {
    // The resume fixture's record, under its own session id.
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
    let listed = list_sessions(home.path(), CWD).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id, id);
    assert_eq!(
        listed[0].title.as_deref(),
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

    let listed: Vec<SessionSummary> = list_sessions(home.path(), CWD).unwrap();
    let ids: Vec<&str> = listed.iter().map(|s| s.session_id.as_str()).collect();
    assert_eq!(ids, [SESSION_B, SESSION_A]);

    // A directory with no records at all lists nothing.
    assert_eq!(list_sessions(home.path(), r"C:\elsewhere").unwrap(), []);
}
