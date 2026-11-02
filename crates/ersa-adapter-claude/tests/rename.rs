//! Renaming a session (PROTO-CLAUDE-CODE-STREAM, Renaming): a live session
//! by the `rename_session` control request, a stopped one by appending one
//! `custom-title` entry to its record. Records are copies of the recorded
//! fixtures in a scratch Claude home, never the real `~/.claude`.

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::{RecordingSink, fixture, fixtures_root};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, AdapterError, SessionConfig};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_adapter_claude::record::{encoded_cwd, list_sessions};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(5);
const CWD: &str = "C:/work";
const SESSION: &str = "d34f0c1b-64ee-437e-a4e3-3487df29a506";

/// The record the CLI kept for the recorded resume run, as bytes.
fn recorded_record() -> Vec<u8> {
    fs::read(fixtures_root().join("2.1.316/record/01-session-resume.jsonl")).unwrap()
}

/// A scratch Claude home holding `record` as session [`SESSION`]'s.
fn home_with(record: &[u8]) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects").join(encoded_cwd(CWD));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{SESSION}.jsonl")), record).unwrap();
    home
}

fn record_file(home: &Path) -> std::path::PathBuf {
    home.join("projects")
        .join(encoded_cwd(CWD))
        .join(format!("{SESSION}.jsonl"))
}

fn adapter(home: &Path) -> ClaudeAdapter {
    ClaudeAdapter::new()
        .with_claude_home(home.to_path_buf())
        .with_handshake_timeout(WAIT)
        .with_stop_grace(Duration::from_millis(200))
}

/// No transport is used to rename a stopped session's record; the record is
/// a local file.
fn no_transport() -> Arc<FakeTransport> {
    Arc::new(FakeTransport::new(Vec::<String>::new()))
}

fn listed_title(home: &Path) -> Option<String> {
    let listed = list_sessions(home, CWD).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    listed.sessions[0].title.clone()
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R22
#[tokio::test]
async fn renaming_a_stopped_session_appends_one_custom_title_and_nothing_else() {
    let original = recorded_record();
    let home = home_with(&original);
    adapter(home.path())
        .rename_record(no_transport(), CWD, SESSION, "Renamed in Ersa")
        .await
        .expect("rename");

    let after = fs::read(record_file(home.path())).unwrap();
    // Everything that was there is there still, byte for byte.
    assert_eq!(&after[..original.len()], &original[..]);
    // Followed by exactly one line: the entry, as the CLI writes it.
    let added = std::str::from_utf8(&after[original.len()..]).unwrap();
    assert_eq!(added.matches('\n').count(), 1, "{added:?}");
    assert!(added.ends_with('\n'));
    assert_eq!(
        serde_json::from_str::<Value>(added.trim_end()).unwrap(),
        json!({"type": "custom-title", "customTitle": "Renamed in Ersa", "sessionId": SESSION})
    );
    // And the session now lists under it, in Ersa and in the CLI's tools.
    assert_eq!(
        listed_title(home.path()).as_deref(),
        Some("Renamed in Ersa")
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R22
#[tokio::test]
async fn the_entry_has_the_shape_the_cli_itself_writes() {
    // The CLI's own entry after a live rename, from fixture 03-session-rename.
    let recorded =
        fs::read_to_string(fixtures_root().join("2.1.316/record/03-session-rename.jsonl")).unwrap();
    let cli_entry = recorded
        .lines()
        .find(|l| l.contains("\"custom-title\""))
        .expect("the CLI wrote a custom-title entry");
    let session = serde_json::from_str::<Value>(cli_entry).unwrap()["sessionId"]
        .as_str()
        .unwrap()
        .to_string();
    let title = serde_json::from_str::<Value>(cli_entry).unwrap()["customTitle"]
        .as_str()
        .unwrap()
        .to_string();

    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("projects").join(encoded_cwd(CWD));
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{session}.jsonl"));
    fs::write(&file, "").unwrap();
    adapter(home.path())
        .rename_record(no_transport(), CWD, &session, &title)
        .await
        .expect("rename");
    // The same line, to the byte.
    assert_eq!(fs::read_to_string(&file).unwrap(), format!("{cli_entry}\n"));
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R22
#[tokio::test]
async fn a_record_whose_last_line_is_still_being_written_is_left_alone() {
    // A CLI part-way through a line: the entry would split it.
    let mut original = recorded_record();
    original.extend_from_slice(b"{\"type\":\"assistant\",\"mess");
    let home = home_with(&original);
    let error = adapter(home.path())
        .rename_record(no_transport(), CWD, SESSION, "Not now")
        .await
        .unwrap_err();
    assert!(
        matches!(&error, AdapterError::Io(e) if e.kind() == std::io::ErrorKind::ResourceBusy),
        "{error}"
    );
    assert_eq!(fs::read(record_file(home.path())).unwrap(), original);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R22
#[tokio::test]
async fn a_title_with_a_line_break_or_quotes_stays_one_entry() {
    let original = recorded_record();
    let home = home_with(&original);
    let title = "two\nlines \"quoted\"";
    adapter(home.path())
        .rename_record(no_transport(), CWD, SESSION, title)
        .await
        .expect("rename");
    let after = fs::read(record_file(home.path())).unwrap();
    let added = std::str::from_utf8(&after[original.len()..]).unwrap();
    assert_eq!(added.matches('\n').count(), 1, "{added:?}");
    assert_eq!(
        serde_json::from_str::<Value>(added.trim_end()).unwrap()["customTitle"],
        title
    );
}

#[tokio::test]
async fn a_session_without_a_record_is_not_renamed() {
    let home = tempfile::tempdir().unwrap();
    let error = adapter(home.path())
        .rename_record(no_transport(), CWD, SESSION, "Nowhere")
        .await
        .unwrap_err();
    assert!(
        matches!(&error, AdapterError::Io(e) if e.kind() == std::io::ErrorKind::NotFound),
        "{error}"
    );
    // Nothing was created on the way.
    assert!(!home.path().join("projects").exists());
    // And an id that is not a session id never becomes a path.
    let error = adapter(home.path())
        .rename_record(no_transport(), CWD, "../../secrets", "x")
        .await
        .unwrap_err();
    assert!(
        matches!(&error, AdapterError::Io(e) if e.kind() == std::io::ErrorKind::InvalidInput),
        "{error}"
    );
}

#[tokio::test]
async fn a_rename_is_listed_at_once_though_the_title_was_read_before() {
    let home = home_with(&recorded_record());
    let adapter = adapter(home.path());
    let before = adapter
        .summarize_record(no_transport(), CWD, SESSION)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(before.title.as_deref(), Some("Fresh title"));
    adapter
        .rename_record(no_transport(), CWD, SESSION, "Fresh title")
        .await
        .unwrap();
    let after = adapter
        .summarize_record(no_transport(), CWD, SESSION)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.title.as_deref(), Some("Fresh title"));
}

/// A stub CLI that answers `initialize`, then answers the next stdin line
/// (the rename) with `answer`, built from that line's `request_id`.
fn renaming_stub(answer: fn(&str) -> Value) -> FakeTransport {
    let init_answer = fixture("11-stream-permission-prompt-tool-stdio").stdout[0].clone();
    let rename = fixture("2.1.316/03-session-rename");
    // The CLI's own lines around the rename: its announcement of the title.
    let announced = rename
        .stdout
        .iter()
        .find(|l| l.contains("session_title_changed"))
        .expect("the recording announces the title")
        .clone();
    // The stub's stdout is fixed in advance, so the answer echoes the id the
    // driver chooses for its first request after `initialize`.
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([init_answer])
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([announced, answer("ersa-rename-2").to_string()])
        .hold_open()
}

fn config() -> SessionConfig {
    SessionConfig {
        cwd: CWD.into(),
        permission_mode: "default".into(),
        model: None,
        project_trusted: false,
        project_path: String::new(),
        extra: Value::Null,
    }
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R22
#[tokio::test]
async fn renaming_a_live_session_sends_rename_session_and_writes_no_record() {
    let home = home_with(&recorded_record());
    let before = fs::read(record_file(home.path())).unwrap();
    let transport = renaming_stub(
        |id| json!({"type": "control_response", "response": {"subtype": "success", "request_id": id}}),
    );
    let sink = Arc::new(RecordingSink::new());
    let session = adapter(home.path())
        .start(Arc::new(transport.clone()), config(), sink)
        .await
        .expect("start");
    tokio::time::timeout(WAIT, session.rename(SESSION, "Renamed live"))
        .await
        .expect("answered in time")
        .expect("rename");

    let sent: Value = serde_json::from_str(&transport.stdin_lines()[1]).unwrap();
    assert_eq!(
        sent,
        json!({"type": "control_request", "request_id": "ersa-rename-2", "request": {
            "subtype": "rename_session", "title": "Renamed live", "source": "host",
            "session_id": SESSION}})
    );
    // The CLI writes its own entry; Ersa writes none.
    assert_eq!(fs::read(record_file(home.path())).unwrap(), before);
    assert!(transport.stub_failures().is_empty());
    session.stop().await.unwrap();
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R22
#[tokio::test]
async fn a_refused_live_rename_is_an_error() {
    let home = home_with(&recorded_record());
    let transport = renaming_stub(|id| {
        json!({"type": "control_response", "response": {"subtype": "error", "request_id": id,
            "error": "rename_session: title must be a string"}})
    });
    let session = adapter(home.path())
        .start(
            Arc::new(transport),
            config(),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");
    let error = tokio::time::timeout(WAIT, session.rename(SESSION, "x"))
        .await
        .expect("answered in time")
        .unwrap_err();
    assert!(
        error.to_string().contains("title must be a string"),
        "{error}"
    );
    session.stop().await.unwrap();
}

#[tokio::test]
async fn a_live_rename_ends_when_the_cli_exits_without_answering() {
    let home = home_with(&recorded_record());
    let init_answer = fixture("11-stream-permission-prompt-tool-stdio").stdout[0].clone();
    // Answers `initialize`, takes the rename, then exits.
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([init_answer])
        .wait_for_stdin(|_| Ok(()))
        .with_exit_code(1);
    let session = adapter(home.path())
        .start(
            Arc::new(transport),
            config(),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");
    let error = tokio::time::timeout(WAIT, session.rename(SESSION, "x"))
        .await
        .expect("ends with the CLI")
        .unwrap_err();
    assert!(matches!(error, AdapterError::NotRunning), "{error}");
}

#[tokio::test]
async fn a_live_rename_the_cli_never_answers_gives_up() {
    let home = home_with(&recorded_record());
    let init_answer = fixture("11-stream-permission-prompt-tool-stdio").stdout[0].clone();
    // Answers `initialize`, takes the rename, and says nothing more.
    let transport = FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([init_answer])
        .hold_open();
    let session = adapter(home.path())
        .with_answer_timeout(Duration::from_millis(300))
        .start(
            Arc::new(transport),
            config(),
            Arc::new(RecordingSink::new()),
        )
        .await
        .expect("start");
    let error = tokio::time::timeout(WAIT, session.rename(SESSION, "x"))
        .await
        .expect("gives up in time")
        .unwrap_err();
    assert!(error.to_string().contains("did not answer"), "{error}");
    session.stop().await.unwrap();
}
