//! Dormant sessions: a session whose backend has no registered adapter is
//! hidden while Core runs without the adapter, kept in storage, and listed
//! again once Core starts with it (PROTO-CORE-API-R66 to R68, ADR 0031).
//! Each test restarts a test Core over the same data directory with a
//! different set of adapters.

mod support;

use std::sync::Arc;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Records};
use support::{Client, TestCore, kind, project, start_with_adapters, tempdir, workspace};

/// Stops `core` the way the Shell does, and hands back its directory for
/// the next Core to start over.
async fn shut_down(core: TestCore, client: &mut Client) -> tempfile::TempDir {
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    dir
}

async fn create(client: &mut Client, workspace_id: &Value, backend: &str) -> String {
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": backend }),
        )
        .await;
    session["id"].as_str().unwrap().to_string()
}

fn listed_ids(sessions: &Value) -> Vec<String> {
    sessions
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

// verifies: PROTO-CORE-API-R66
// verifies: PROTO-CORE-API-R67
#[tokio::test(flavor = "multi_thread")]
async fn a_session_without_its_adapter_is_hidden_and_returns_with_it() {
    let records = Records::default();
    let with_fake = || -> Vec<Arc<dyn Adapter>> {
        vec![Arc::new(FakeAdapter::with_records(Arc::clone(&records)))]
    };
    let core = start_with_adapters(tempdir(), |_| {}, with_fake()).await;
    let mut client = core.client().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = create(&mut client, &workspace_id, fake::BACKEND).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    // Idle, since a busy session can't be archived (PROTO-CORE-API-R77).
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let listed = client.result("session.list", json!({})).await;
        if listed
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == id.as_str() && s["state"] == "idle")
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{id} never idle");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    // Arranged, so its arrangement can be seen to come back with it.
    let section = client
        .result("section.create", json!({ "name": "Kept" }))
        .await;
    client
        .result(
            "session.arrange",
            json!({ "id": id, "pinned": true, "section_id": section["id"] }),
        )
        .await;
    client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;
    let data_dir = core.data_dir();
    let dir = shut_down(core, &mut client).await;
    let facts = facts_of(&data_dir, &id);
    assert!(!facts.is_empty());

    // A Core without the fake adapter does not know the session at all.
    let other: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::named("other"))];
    let core = start_with_adapters(dir, |_| {}, other).await;
    let mut client = core.client().await;
    let listed = client.result("session.list", json!({})).await;
    assert!(!listed_ids(&listed).contains(&id), "{listed}");
    let by_id = [
        ("session.subscribe", json!({ "id": id, "from_seq": 0 })),
        ("session.raw_log", json!({ "id": id })),
        ("session.stop", json!({ "id": id })),
        ("session.send", json!({ "id": id, "text": "again" })),
        ("session.set_title", json!({ "id": id, "title": "Renamed" })),
        (
            "session.set_archived",
            json!({ "id": id, "archived": false }),
        ),
        ("session.arrange", json!({ "id": id, "pinned": false })),
        ("session.fork", json!({ "id": id })),
        ("session.delete", json!({ "id": id, "token": "t" })),
        ("session.remove", json!({ "id": id })),
    ];
    for (method, params) in by_id {
        let error = client.error(method, params).await;
        assert_eq!(kind(&error), "not_found", "{method}: {error}");
    }
    let dir = shut_down(core, &mut client).await;
    // What Core knew of it is kept, and nothing was added.
    assert_eq!(facts_of(&data_dir, &id), facts);

    // With the adapter back, it is listed again as it was arranged, and
    // reads as before: its history from the record, then its stop.
    let core = start_with_adapters(dir, |_| {}, with_fake()).await;
    let mut client = core.client().await;
    let listed = client.result("session.list", json!({})).await;
    let row = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id.as_str())
        .unwrap_or_else(|| panic!("{id} listed again: {listed}"))
        .clone();
    assert_eq!(row["pinned"], true);
    assert_eq!(row["section_id"], section["id"]);
    assert_eq!(row["archived"], true);
    assert_eq!(row["workspace_id"], workspace_id);
    assert_eq!(row["state"], "ended");
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["event"]["type"].as_str().unwrap())
        .collect();
    assert_eq!(types[..2], ["user.message", "assistant.text"]);
    assert_eq!(events.last().unwrap()["event"]["reason"], "user_stopped");
}

/// The types of the app log's facts about session `id`, oldest first. Read
/// once Core has shut down, which flushes them.
fn facts_of(data_dir: &std::path::Path, id: &str) -> Vec<String> {
    ersa_core::app_log::read_entries(data_dir)
        .unwrap()
        .into_iter()
        .filter(|e| e.session_id == id)
        .map(|e| serde_json::to_value(&e.fact).unwrap()["type"].to_string())
        .collect()
}

// verifies: PROTO-CORE-API-R68
#[tokio::test(flavor = "multi_thread")]
async fn a_dormant_session_still_holds_its_workspace() {
    let both = || -> Vec<Arc<dyn Adapter>> {
        vec![
            Arc::new(FakeAdapter::new(1)),
            Arc::new(FakeAdapter::named("other")),
        ]
    };
    let core = start_with_adapters(tempdir(), |_| {}, both()).await;
    let mut client = core.client().await;
    // An isolated workspace with a session of each backend.
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let shown = create(&mut client, &workspace_id, fake::BACKEND).await;
    let dormant = create(&mut client, &workspace_id, "other").await;
    // A second project with an "other" session in its own folder.
    let second = core.dir.path().join("second");
    std::fs::create_dir_all(&second).unwrap();
    let second = project(&mut client, &second).await;
    let workspaces = client
        .result("workspace.list", json!({ "project_id": second["id"] }))
        .await;
    let root = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == "root")
        .unwrap()
        .clone();
    let in_root = create(&mut client, &root["id"], "other").await;
    let dir = shut_down(core, &mut client).await;

    let only_fake: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, only_fake).await;
    let mut client = core.client().await;
    let listed = client.result("session.list", json!({})).await;
    assert_eq!(listed_ids(&listed), std::slice::from_ref(&shown));
    for id in [&dormant, &in_root] {
        let error = client.error("session.remove", json!({ "id": id })).await;
        assert_eq!(kind(&error), "not_found");
    }

    // `workspace.changes` counts it.
    let changes = client
        .result("workspace.changes", json!({ "id": workspace_id }))
        .await;
    assert_eq!(changes["sessions"], 2, "{changes}");

    // Removing the workspace, or the project whose own folder it uses, is
    // refused.
    let error = client
        .error("workspace.remove", json!({ "id": workspace_id }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let error = client
        .error("project.remove", json!({ "id": second["id"] }))
        .await;
    assert_eq!(kind(&error), "invalid_state");

    // Deleting the other session keeps the workspace for it.
    let deleted = client
        .result(
            "session.delete",
            json!({ "id": shown, "token": changes["token"] }),
        )
        .await;
    assert_eq!(deleted["workspace_deleted"], false, "{deleted}");
    let changes = client
        .result("workspace.changes", json!({ "id": workspace_id }))
        .await;
    assert_eq!(changes["sessions"], 1, "{changes}");
}
