//! Deleting a session's record (PROTO-CLAUDE-CODE-STREAM, Deleting): its
//! record file and the folder of the same name beside it go, nothing else of
//! the CLI's does, and nothing goes while another process has the session
//! open or the registry cannot say. Every file is in a scratch Claude home,
//! never the real `~/.claude`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, OpenElsewhere, RecordDeletion};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_adapter_claude::record::encoded_cwd;
use serde_json::json;

const CWD: &str = "C:/work";
const SESSION: &str = "d34f0c1b-64ee-437e-a4e3-3487df29a506";
const OTHER: &str = "0989e382-5a8f-4fdf-9738-bf02b1f049e1";

fn project_dir(home: &Path) -> PathBuf {
    home.join("projects").join(encoded_cwd(CWD))
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// A scratch Claude home holding, under [`CWD`]'s directory, a record and
/// its folder for [`SESSION`] and for [`OTHER`], and the CLI's other files.
fn home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = project_dir(home.path());
    for id in [SESSION, OTHER] {
        write(&dir.join(format!("{id}.jsonl")), "{}\n");
        write(&dir.join(id).join("subagents/agent-1.jsonl"), "{}\n");
        write(&dir.join(id).join("tool-results/r.txt"), "result\n");
    }
    write(&home.path().join("history.jsonl"), "{}\n");
    write(&dir.join("memory.md"), "notes\n");
    home
}

/// Every file under `root`, relative to it, sorted.
fn files(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                found.push(relative.display().to_string().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

fn adapter(home: &Path) -> ClaudeAdapter {
    ClaudeAdapter::new().with_claude_home(home.to_path_buf())
}

/// No transport is used: the record is a local file.
fn no_transport() -> Arc<FakeTransport> {
    Arc::new(FakeTransport::new(Vec::<String>::new()))
}

/// A registry file saying process `pid` has `session` open.
fn register(home: &Path, pid: u32, session: &str) {
    write(
        &home.join("sessions").join(format!("{pid}.json")),
        &json!({ "pid": pid, "sessionId": session, "cwd": CWD, "entrypoint": "cli" }).to_string(),
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R31
#[tokio::test]
async fn deleting_takes_the_record_file_and_its_folder_and_nothing_else() {
    let home = home();
    let before = files(home.path());
    let deleted = adapter(home.path())
        .delete_record(no_transport(), CWD, SESSION)
        .await
        .expect("delete");
    assert_eq!(deleted, RecordDeletion::Deleted);

    let encoded = encoded_cwd(CWD);
    let expected: Vec<String> = before
        .into_iter()
        .filter(|f| !f.starts_with(&format!("projects/{encoded}/{SESSION}")))
        .collect();
    assert_eq!(files(home.path()), expected);
    // The other session's record and folder, the CLI's history and the
    // project's own files are all still there.
    assert!(expected.contains(&format!("projects/{encoded}/{OTHER}.jsonl")));
    assert!(expected.contains(&format!("projects/{encoded}/{OTHER}/tool-results/r.txt")));
    assert!(expected.contains(&"history.jsonl".to_string()));
    assert!(!project_dir(home.path()).join(SESSION).exists());
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R31
#[tokio::test]
async fn a_record_filed_under_another_spelling_of_the_folder_is_found_and_deleted() {
    let home = tempfile::tempdir().unwrap();
    let elsewhere = home.path().join("projects").join("c--work");
    write(&elsewhere.join(format!("{SESSION}.jsonl")), "{}\n");
    write(&elsewhere.join(SESSION).join("subagents/a.jsonl"), "{}\n");
    write(&elsewhere.join("kept.txt"), "kept\n");
    let deleted = adapter(home.path())
        .delete_record(no_transport(), CWD, SESSION)
        .await
        .expect("delete");
    assert_eq!(deleted, RecordDeletion::Deleted);
    assert_eq!(files(home.path()), ["projects/c--work/kept.txt"]);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R33
#[tokio::test]
async fn a_record_already_gone_counts_as_deleted_and_its_folder_left_behind_goes() {
    let home = home();
    let dir = project_dir(home.path());
    fs::remove_file(dir.join(format!("{SESSION}.jsonl"))).unwrap();
    let deleted = adapter(home.path())
        .delete_record(no_transport(), CWD, SESSION)
        .await
        .expect("delete");
    assert_eq!(deleted, RecordDeletion::Deleted);
    assert!(!dir.join(SESSION).exists());

    // And with nothing of it left at all, once more.
    let again = adapter(home.path())
        .delete_record(no_transport(), CWD, SESSION)
        .await
        .expect("delete again");
    assert_eq!(again, RecordDeletion::Deleted);
    assert!(dir.join(format!("{OTHER}.jsonl")).exists());
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R32
#[tokio::test]
async fn a_session_another_process_has_open_is_refused_with_its_pid_and_nothing_deleted() {
    let home = home();
    // This test process stands in for a running CLI.
    let pid = std::process::id();
    register(home.path(), pid, SESSION);
    let before = files(home.path());
    let deleted = adapter(home.path())
        .delete_record(no_transport(), CWD, SESSION)
        .await
        .expect("answered");
    assert_eq!(deleted, RecordDeletion::Refused(OpenElsewhere::Yes { pid }));
    assert_eq!(files(home.path()), before);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R32
#[tokio::test]
async fn a_registry_that_cannot_say_refuses_and_deletes_nothing() {
    let home = home();
    // A registry file of this running process that is not one the adapter
    // can read: it may name the session.
    write(
        &home
            .path()
            .join("sessions")
            .join(format!("{}.json", std::process::id())),
        "not json",
    );
    let before = files(home.path());
    let deleted = adapter(home.path())
        .delete_record(no_transport(), CWD, SESSION)
        .await
        .expect("answered");
    assert_eq!(deleted, RecordDeletion::Refused(OpenElsewhere::Unknown));
    assert_eq!(files(home.path()), before);
}
