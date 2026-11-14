//! Resuming a stopped session (FLOW-RESUME-SESSION), through the Core API
//! with the fake adapter (`support/fake.rs`), whose session records outlive
//! a Core as Claude Code's do: `session.resume`, a send to a session that is
//! not running, a session whose folder is gone, and a session a Core that
//! died left running.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Records, Stats};
use support::{Client, TestCore, kind, start_with_adapters, tempdir, workspace};

async fn start_fake(adapter: FakeAdapter) -> (TestCore, Arc<Stats>) {
    let stats = Arc::clone(&adapter.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    (
        start_with_adapters(tempdir(), |_| {}, adapters).await,
        stats,
    )
}

/// A session of the fake backend in a new workspace, which has had one
/// turn ("hello") and has been stopped; returns its id.
async fn stopped_session(core: &TestCore, client: &mut Client) -> String {
    let workspace_id = workspace(client, core.dir.path()).await;
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    id
}

async fn session_row(client: &mut Client, id: &str) -> Value {
    let sessions = client.result("session.list", json!({})).await;
    sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("session {id} listed"))
}

/// The configurations of the backend's first `n` starts, once there are as
/// many: Core starts a backend after it answers.
async fn started(stats: &Stats, n: usize) -> Vec<ersa_adapter::SessionConfig> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let configs = stats.configs.lock().unwrap().clone();
        if configs.len() >= n {
            return configs;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} starts",
            configs.len()
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

/// The one record the fake backend keeps: its id for the session.
fn only_record(records: &Records) -> String {
    let records = records.lock().unwrap();
    assert_eq!(records.len(), 1, "{:?}", records.keys());
    records.keys().next().unwrap().clone()
}

/// The text of each message event, with where it came from.
fn messages(events: &[Value]) -> Vec<(String, String)> {
    events
        .iter()
        .filter(|e| {
            matches!(
                e["event"]["type"].as_str(),
                Some("user.message" | "assistant.text")
            )
        })
        .map(|e| {
            (
                e["source"]["input"].as_str().unwrap().to_string(),
                e["event"]["text"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

// verifies: FLOW-RESUME-SESSION-R01
#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_session_continues_its_record_in_its_own_folder() {
    let records = Records::default();
    let (core, stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    let backend_id = only_record(&records);
    // Open, as a user reading the stopped session is.
    let subscribed = client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let before = client.events_until("session.ended").await;

    let resumed = client.result("session.resume", json!({ "id": id })).await;
    assert_eq!(resumed["id"], id.as_str());
    assert_eq!(resumed["state"], "starting");
    let configs = started(&stats, 2).await;
    // Its backend id, in its folder: the backend continues that record.
    assert_eq!(configs[1].resume.as_deref(), Some(backend_id.as_str()));
    assert_eq!(configs[1].backend_session_id, None);
    assert_eq!(configs[1].cwd, configs[0].cwd);
    assert_eq!(configs[1].permission_mode, configs[0].permission_mode);

    // Resuming a running session leaves it as it is.
    client.result("session.resume", json!({ "id": id })).await;

    // The view the user reads goes on: the record, then the new run.
    client
        .result("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let after = client.events_until("assistant.text").await;
    let all: Vec<Value> = before.iter().chain(&after).cloned().collect();
    let seqs: Vec<u64> = all.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (0..all.len() as u64).collect::<Vec<_>>());
    assert_eq!(
        messages(&all),
        [
            ("record".into(), "hello".into()),
            ("record".into(), "echo: hello".into()),
            ("live".into(), "again".into()),
            ("live".into(), "echo: again".into()),
        ]
    );
    let view = client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await["view"]
        .clone();
    assert_eq!(view, subscribed["view"], "the same view");
    // Started once, by the first resume: by now any second start would
    // have reached the backend.
    assert_eq!(stats.starts.load(Ordering::SeqCst), 2, "started once");
    // One record holds both turns.
    assert_eq!(only_record(&records), backend_id);
    assert_eq!(records.lock().unwrap()[&backend_id].len(), 4);
}

/// A session whose backend has no record of it yet (stopped before its
/// first message) starts again as a new one, under the id Core stored.
// verifies: FLOW-RESUME-SESSION-R01
#[tokio::test(flavor = "multi_thread")]
async fn a_session_with_no_record_yet_starts_again_as_new_under_its_id() {
    let records = Records::default();
    let (core, stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    let backend_id = only_record(&records);
    // As Claude Code before a session's first message: no record.
    records.lock().unwrap().clear();

    client.result("session.resume", json!({ "id": id })).await;
    let configs = started(&stats, 2).await;
    assert_eq!(configs[1].resume, None);
    assert_eq!(
        configs[1].backend_session_id.as_deref(),
        Some(backend_id.as_str())
    );
    assert_eq!(configs[1].fork_of, None);
}

// verifies: FLOW-RESUME-SESSION-R02
#[tokio::test(flavor = "multi_thread")]
async fn a_send_to_a_stopped_session_resumes_it_reporting_starting() {
    let records = Records::default();
    // Long enough for a progress report, which goes out at most once a
    // second, to land while the backend starts.
    let mut adapter = FakeAdapter::slow_start(std::time::Duration::from_millis(2500));
    adapter.records = Arc::clone(&records);
    let (core, stats) = start_fake(adapter).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    let backend_id = only_record(&records);
    client.clear_notifications();

    let request = client
        .request("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let starting = client
        .notification_where(|n| {
            n["method"] == "session.send_progress" && n["params"]["phase"] == "starting"
        })
        .await;
    assert_eq!(starting["params"]["session_id"], id.as_str());
    assert_eq!(starting["params"]["phase"], "starting");
    let sent = client.response(request).await;
    assert!(sent["result"].is_object(), "{sent:#}");

    assert_eq!(stats.starts.load(Ordering::SeqCst), 2);
    let configs = stats.configs.lock().unwrap().clone();
    assert_eq!(configs[1].resume.as_deref(), Some(backend_id.as_str()));
    let texts: Vec<String> = stats
        .messages
        .lock()
        .unwrap()
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert_eq!(texts, ["hello", "again"]);
    assert_eq!(records.lock().unwrap()[&backend_id].len(), 4);
}

// verifies: FLOW-RESUME-SESSION-R03
// verifies: PROTO-CORE-API-R43
#[tokio::test(flavor = "multi_thread")]
async fn a_session_whose_folder_is_gone_is_neither_resumed_nor_sent_to() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    client
        .result("session.arrange", json!({ "id": id, "pinned": true }))
        .await;
    let folder = stats.configs.lock().unwrap()[0].cwd.clone();
    std::fs::remove_dir_all(&folder).unwrap();
    let before = session_row(&mut client, &id).await;

    for (method, params) in [
        ("session.resume", json!({ "id": id })),
        ("session.send", json!({ "id": id, "text": "again" })),
    ] {
        let error = client.error(method, params).await;
        assert_eq!(kind(&error), "invalid_state", "{method}");
        assert_eq!(error["data"]["reason"], "folder_gone", "{method}");
        assert_eq!(
            stats.starts.load(Ordering::SeqCst),
            1,
            "{method} started nothing"
        );
        let row = session_row(&mut client, &id).await;
        assert_eq!(row["state"], "ended");
        assert_eq!(row["pinned"], true);
        assert_eq!(row["workspace_id"], before["workspace_id"]);
        assert_eq!(row["section_id"], before["section_id"]);
    }
    assert_eq!(
        stats.messages.lock().unwrap().len(),
        1,
        "only the first message"
    );

    // An archived one stays archived (PROTO-CORE-API-R43).
    client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;
    let error = client.error("session.resume", json!({ "id": id })).await;
    assert_eq!(error["data"]["reason"], "folder_gone");
    assert_eq!(session_row(&mut client, &id).await["archived"], true);
}

/// An accepted resume of an archived session unarchives it before its
/// backend starts.
// verifies: PROTO-CORE-API-R43
#[tokio::test(flavor = "multi_thread")]
async fn resuming_an_archived_session_unarchives_it() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;
    let resumed = client.result("session.resume", json!({ "id": id })).await;
    assert_eq!(resumed["archived"], false);
    assert_eq!(resumed["state"], "starting");
    started(&stats, 2).await;
}

// verifies: FLOW-RESUME-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_session_live_when_core_died_ends_its_transcript_and_resumes() {
    let records = Records::default();
    let (core, _) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let id = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    let backend_id = only_record(&records);
    drop(client);
    // No shutdown: the server stops with the session still live.
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;

    let adapter = FakeAdapter::with_records(Arc::clone(&records));
    let stats = Arc::clone(&adapter.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    assert_eq!(session_row(&mut client, &id).await["state"], "ended");
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    assert_eq!(events.last().unwrap()["event"]["reason"], "error");

    // Stopped, and resumable: a message continues the same record.
    client
        .result("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let configs = started(&stats, 1).await;
    assert_eq!(configs[0].resume.as_deref(), Some(backend_id.as_str()));
    let after = client.events_until("assistant.text").await;
    assert_eq!(
        messages(&after),
        [
            ("live".into(), "again".into()),
            ("live".into(), "echo: again".into()),
        ]
    );
}
