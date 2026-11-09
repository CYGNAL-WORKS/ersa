//! A new session starts under an id Core chose (`--session-id`), and is
//! renamed before its first message like any live session
//! (PROTO-CLAUDE-CODE-STREAM, Invocation and Renaming). The CLI's side is
//! fixture 2.1.316/09-session-id-rename: `--session-id`, `initialize`,
//! `rename_session` before the first message, then one message.

mod common;

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture, fixtures_root};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterError, Direction, Projector, RawLine, SessionConfig};
use ersa_adapter_claude::record::{encoded_cwd, is_session_id};
use ersa_adapter_claude::{ClaudeAdapter, ClaudeProjector, session_id_args};
use serde_json::{Value, json};

const FIXTURE: &str = "2.1.316/09-session-id-rename";
const TITLE: &str = "Named before the first message";
const CWD: &str = "C:/work";
const WAIT: Duration = Duration::from_secs(5);

fn adapter() -> ClaudeAdapter {
    ClaudeAdapter::new()
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
}

fn config(backend_session_id: Option<&str>, extra: Value) -> SessionConfig {
    SessionConfig {
        cwd: CWD.into(),
        permission_mode: "default".into(),
        backend_session_id: backend_session_id.map(String::from),
        extra,
        ..SessionConfig::default()
    }
}

/// The value that follows `flag` in `args`, if the flag is there.
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).map(String::as_str)
}

fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap()
}

/// The fixture's recorded command line and session id.
fn recorded_meta() -> Value {
    let meta = fixtures_root().join(format!("{FIXTURE}.meta.json"));
    parse(&fs::read_to_string(meta).unwrap())
}

/// A stub CLI replaying the fixture: it answers `initialize`, then the
/// rename (with the answer's id rewritten to the one the driver chose), and
/// then the first message.
fn stub() -> FakeTransport {
    let stdout = fixture(FIXTURE).stdout;
    let mut answer = parse(&stdout[2]);
    assert_eq!(answer["response"]["request_id"], "ersa-rename_session-1");
    answer["response"]["request_id"] = json!("ersa-rename-2");
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|line| match parse(line)["request"]["subtype"].as_str() {
            Some("initialize") => Ok(()),
            _ => Err(format!("expected initialize, got {line}")),
        })
        .then_stdout([&stdout[0]])
        .wait_for_stdin(|line| match parse(line)["request"]["subtype"].as_str() {
            Some("rename_session") => Ok(()),
            _ => Err(format!("expected rename_session, got {line}")),
        })
        .then_stdout([stdout[1].clone(), answer.to_string()])
        .wait_for_stdin(|line| match parse(line)["type"].as_str() {
            Some("user") => Ok(()),
            _ => Err(format!("expected the first message, got {line}")),
        })
        .then_stdout(&stdout[3..])
        .hold_open()
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R35
#[tokio::test]
async fn a_new_session_starts_under_the_id_core_chose() {
    let id = recorded_meta()["session_id"].as_str().unwrap().to_string();
    let transport = stub();
    let session = adapter()
        .start(
            Arc::new(transport.clone()),
            config(Some(&id), Value::Null),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");
    let args = transport.spawned()[0].args.clone();
    assert_eq!(
        flag_value(&args, "--session-id"),
        Some(id.as_str()),
        "{args:?}"
    );
    assert_eq!(flag_value(&args, "--resume"), None, "{args:?}");
    session.stop().await.unwrap();

    // The recorded CLI was started the same way, and its `system/init`
    // reports the id it was given.
    let recorded = recorded_meta();
    let recorded_args: Vec<String> = serde_json::from_value(recorded["args"].clone()).unwrap();
    assert_eq!(
        flag_value(&recorded_args, "--session-id"),
        Some(id.as_str())
    );
    let init = fixture(FIXTURE)
        .stdout
        .iter()
        .map(|l| parse(l))
        .find(|m| m["type"] == "system" && m["subtype"] == "init")
        .expect("the fixture has a system/init");
    assert_eq!(init["session_id"], id.as_str());
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R35
#[test]
fn every_new_session_gets_a_fresh_id_the_cli_takes() {
    let adapter = adapter();
    let (a, b) = (
        adapter.new_session_id().unwrap(),
        adapter.new_session_id().unwrap(),
    );
    assert!(is_session_id(&a), "{a}");
    assert!(is_session_id(&b), "{b}");
    assert_ne!(a, b);
    // No id chosen, no flag: the CLI names the session itself.
    assert!(
        session_id_args(&config(None, Value::Null))
            .unwrap()
            .is_empty()
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R35
#[tokio::test]
async fn a_chosen_id_never_goes_with_resume_or_as_anything_but_an_id() {
    let id = recorded_meta()["session_id"].as_str().unwrap().to_string();
    let resume = json!({ "resume": "d34f0c1b-64ee-437e-a4e3-3487df29a506" });
    for (chosen, extra) in [
        (id.as_str(), resume),
        ("--dangerously-skip-permissions", Value::Null),
        ("abc", Value::Null),
    ] {
        let transport = stub();
        let result = adapter()
            .start(
                Arc::new(transport.clone()),
                config(Some(chosen), extra.clone()),
                Arc::new(RecordingSink::new()),
            )
            .await;
        assert!(
            matches!(result, Err(AdapterError::Protocol(_))),
            "{chosen:?} with {extra} was accepted"
        );
        assert!(transport.spawned().is_empty(), "{chosen:?} spawned the CLI");
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R35
#[tokio::test]
async fn a_stored_id_with_no_record_yet_reads_as_an_empty_session() {
    // The CLI writes the record with the first message; until then the id
    // names nothing on disk, in a home with other sessions or none at all.
    let id = recorded_meta()["session_id"].as_str().unwrap().to_string();
    let empty = tempfile::tempdir().unwrap();
    let busy = tempfile::tempdir().unwrap();
    let other = busy.path().join("projects").join(encoded_cwd(CWD));
    fs::create_dir_all(&other).unwrap();
    fs::write(
        other.join("d34f0c1b-64ee-437e-a4e3-3487df29a506.jsonl"),
        fs::read(fixtures_root().join("2.1.316/record/01-session-resume.jsonl")).unwrap(),
    )
    .unwrap();
    for home in [empty.path(), busy.path()] {
        let adapter = adapter().with_claude_home(home.to_path_buf());
        let transport = Arc::new(FakeTransport::new(Vec::<String>::new()));
        let events = adapter
            .read_record(transport.clone(), CWD, &id)
            .await
            .expect("an empty history, not an error");
        assert!(events.is_empty(), "{events:?}");
        let summary = adapter.summarize_record(transport.clone(), CWD, &id).await;
        assert_eq!(summary.unwrap(), None);
        let version = adapter.record_version(transport, CWD, &id).await;
        assert_eq!(version.unwrap(), None);
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R36
#[tokio::test]
async fn a_session_is_renamed_before_its_first_message() {
    let id = recorded_meta()["session_id"].as_str().unwrap().to_string();
    let transport = stub();
    let sink = Arc::new(RecordingSink::new());
    let session = adapter()
        .start(
            Arc::new(transport.clone()),
            config(Some(&id), Value::Null),
            sink.clone(),
        )
        .await
        .expect("start");
    tokio::time::timeout(WAIT, session.rename(&id, TITLE))
        .await
        .expect("answered in time")
        .expect("rename");
    session
        .send_user("Reply with the single word: ok")
        .await
        .unwrap();

    let sent: Vec<Value> = transport.stdin_lines().iter().map(|l| parse(l)).collect();
    assert_eq!(sent[0]["request"]["subtype"], "initialize");
    assert_eq!(
        sent[1],
        json!({"type": "control_request", "request_id": "ersa-rename-2", "request": {
            "subtype": "rename_session", "title": TITLE, "source": "host", "session_id": id}})
    );
    assert_eq!(sent[2]["type"], "user");
    assert!(
        transport.stub_failures().is_empty(),
        "{:?}",
        transport.stub_failures()
    );
    session.stop().await.unwrap();

    // The CLI held the name and wrote it with the record the first message
    // created, so the record lists the session under it.
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects").join(encoded_cwd(CWD));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{id}.jsonl")),
        fs::read(fixtures_root().join("2.1.316/record/09-session-id-rename.jsonl")).unwrap(),
    )
    .unwrap();
    let summary = adapter()
        .with_claude_home(home.path().to_path_buf())
        .summarize_record(Arc::new(FakeTransport::new(Vec::<String>::new())), CWD, &id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.title.as_deref(), Some(TITLE));
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R36
#[test]
fn the_latest_announced_title_is_reported_until_another() {
    let mut projector = ClaudeProjector::new();
    let line = |index: u64, line: &str| RawLine {
        index,
        ts: "2026-11-07T00:00:00Z".into(),
        dir: Direction::Backend,
        line: line.into(),
    };
    let stdout = fixture(FIXTURE).stdout;
    projector.project(&line(0, &stdout[0]));
    assert_eq!(projector.announced_title(), None);
    let events = projector.project(&line(1, &stdout[1]));
    assert_eq!(projector.announced_title(), Some((1, TITLE)));
    // The line itself is still kept, as a passthrough.
    assert_eq!(events.len(), 1);
    // Every later line leaves it, until the next announcement.
    for (n, l) in stdout[2..].iter().enumerate() {
        projector.project(&line(n as u64 + 2, l));
    }
    assert_eq!(projector.announced_title(), Some((1, TITLE)));
    // The same name again is an announcement of its own.
    projector.project(&line(98, &stdout[1]));
    assert_eq!(projector.announced_title(), Some((2, TITLE)));
    let mut next = parse(&stdout[1]);
    next["title"] = json!("Renamed again");
    projector.project(&line(99, &next.to_string()));
    assert_eq!(projector.announced_title(), Some((3, "Renamed again")));
}
