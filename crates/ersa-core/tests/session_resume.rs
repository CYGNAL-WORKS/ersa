//! Resuming a stopped session (FLOW-RESUME-SESSION), through the Core API
//! with the fake adapter (`support/fake.rs`), whose session records outlive
//! a Core as Claude Code's do: `session.resume`, a send to a session that is
//! not running, a session whose folder is gone, and a session a Core that
//! died left running.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ersa_adapter::OpenElsewhere;
use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Records, Stats};
use support::{Client, TestCore, kind, start_with_adapters, tempdir, wait_for_state, workspace};

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
// verifies: PROTO-AGENT-EVENTS-R24
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
    let configs = stats.started(2).await;
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
    let configs = stats.started(2).await;
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

// verifies: FLOW-RESUME-SESSION-R23
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
    stats.started(2).await;
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
    let configs = stats.started(1).await;
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

/// A fork of a stopped session, stopped in turn before its first message,
/// so that, as with Claude Code, which writes a fork's record with that
/// message, it has no record of its own. Returns the original's id, the
/// fork's, and the backend's ids for each.
async fn recordless_fork(
    client: &mut Client,
    core: &TestCore,
    records: &Records,
    stats: &Stats,
) -> (String, String, String, String) {
    let original = stopped_session(core, client).await;
    let original_backend_id = only_record(records);
    let fork = client
        .result("session.fork", json!({ "id": original }))
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let starts = stats.starts.load(Ordering::SeqCst);
    stats.started(starts).await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while records.lock().unwrap().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never started"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let fork_backend_id = records
        .lock()
        .unwrap()
        .keys()
        .find(|k| **k != original_backend_id)
        .unwrap()
        .clone();
    records.lock().unwrap().remove(&fork_backend_id);
    client.result("session.stop", json!({ "id": fork })).await;
    (original, fork, original_backend_id, fork_backend_id)
}

/// A fork stopped before its first message has no record of its own: it
/// starts again as a fork of its original, under its own id.
// verifies: FLOW-RESUME-SESSION-R01
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_with_no_record_yet_is_forked_from_its_original_again() {
    let records = Records::default();
    let (core, stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let (_, fork, original_backend_id, fork_backend_id) =
        recordless_fork(&mut client, &core, &records, &stats).await;

    client.result("session.resume", json!({ "id": fork })).await;
    let configs = stats.started(3).await;
    let again = &configs[2];
    assert_eq!(
        again.fork_of.as_ref().map(|f| f.original.as_str()),
        Some(original_backend_id.as_str())
    );
    assert_eq!(
        again.backend_session_id.as_deref(),
        Some(fork_backend_id.as_str())
    );
    assert_eq!(again.resume, None);
}

/// Resuming a fork with no record of its own copies its original again, so
/// it is refused whenever forking the original would be, with the same
/// answer, by `session.resume` and by a send alike: here while a turn runs
/// in the original, which the copy would take part way (fixture
/// 2.1.316/18), and while another process has the original open.
// verifies: FLOW-RESUME-SESSION-R10
#[tokio::test(flavor = "multi_thread")]
async fn a_recordless_fork_is_resumed_only_when_its_original_can_be_forked() {
    let records = Records::default();
    let fake = FakeAdapter::with_records(Arc::clone(&records));
    let open_elsewhere = Arc::clone(&fake.open_elsewhere);
    let (core, stats) = start_fake(fake).await;
    let mut client = core.client().await;
    let (original, fork, _, _) = recordless_fork(&mut client, &core, &records, &stats).await;

    // Another process has the original open.
    *open_elsewhere.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    let starts = stats.starts.load(Ordering::SeqCst);
    let forking = client
        .error("session.fork", json!({ "id": original }))
        .await;
    assert_eq!(forking["data"]["reason"], "open_elsewhere", "{forking:#}");
    let resuming = client.error("session.resume", json!({ "id": fork })).await;
    assert_eq!(resuming, forking);
    let sending = client
        .error("session.send", json!({ "id": fork, "text": "go on" }))
        .await;
    assert_eq!(sending, forking);
    *open_elsewhere.lock().unwrap() = OpenElsewhere::No;

    // A turn runs in the original.
    client
        .result("session.send", json!({ "id": original, "text": "work" }))
        .await;
    wait_for_state(&mut client, &original, "running").await;
    let starts_now = stats.starts.load(Ordering::SeqCst);
    assert_eq!(starts_now, starts + 1, "only the original resumed");
    let forking = client
        .error("session.fork", json!({ "id": original }))
        .await;
    assert_eq!(kind(&forking), "invalid_state");
    assert_eq!(forking["data"]["reason"], "session_busy", "{forking:#}");
    let resuming = client.error("session.resume", json!({ "id": fork })).await;
    assert_eq!(resuming, forking);
    let sending = client
        .error("session.send", json!({ "id": fork, "text": "go on" }))
        .await;
    assert_eq!(sending, forking);

    // Neither started the fork's backend again.
    assert_eq!(stats.starts.load(Ordering::SeqCst), starts_now);
    assert_eq!(session_row(&mut client, &fork).await["state"], "ended");
}

/// Deletes session `id`, so Core no longer lists it.
async fn delete(client: &mut Client, id: &str) {
    let workspace_id = session_row(client, id).await["workspace_id"].clone();
    let token = client
        .result("workspace.changes", json!({ "id": workspace_id }))
        .await["token"]
        .clone();
    client
        .result("session.delete", json!({ "id": id, "token": token }))
        .await;
}

/// A recordless fork whose original Core no longer lists has no original
/// to check `session.fork`'s conditions on: its backend is asked to fork
/// the original's record all the same, even while, for instance, the
/// registry says another process has that record open, which would refuse
/// a fork of a listed original. Here the record is still there, as when
/// Core forgot the original without deleting it, so the fork resumes.
// verifies: FLOW-RESUME-SESSION-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_recordless_fork_whose_original_is_gone_is_forked_without_its_conditions() {
    let records = Records::default();
    let fake = FakeAdapter::with_records(Arc::clone(&records));
    let open_elsewhere = Arc::clone(&fake.open_elsewhere);
    let (core, stats) = start_fake(fake).await;
    let mut client = core.client().await;
    let (original, fork, original_backend_id, _) =
        recordless_fork(&mut client, &core, &records, &stats).await;
    let record = records.lock().unwrap()[&original_backend_id].clone();
    delete(&mut client, &original).await;
    // Forgotten, not deleted: the backend still has the record.
    records
        .lock()
        .unwrap()
        .insert(original_backend_id.clone(), record);

    *open_elsewhere.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    client
        .result("session.send", json!({ "id": fork, "text": "go on" }))
        .await;
    let configs = stats.started(3).await;
    assert_eq!(
        configs[2].fork_of.as_ref().map(|f| f.original.as_str()),
        Some(original_backend_id.as_str())
    );
    let texts: Vec<String> = stats
        .messages
        .lock()
        .unwrap()
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert_eq!(texts.last().map(String::as_str), Some("go on"));
}

/// A recordless fork whose original was deleted, record and all, is still
/// started from that record; the backend can't fork it, and the answer is
/// the one any backend that could not be started gives: its own words,
/// with no `reason`, and the session in `error`.
// verifies: FLOW-RESUME-SESSION-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_recordless_fork_whose_original_was_deleted_fails_as_a_start_does() {
    let records = Records::default();
    let (core, stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let (original, fork, original_backend_id, _) =
        recordless_fork(&mut client, &core, &records, &stats).await;
    delete(&mut client, &original).await;
    assert!(!records.lock().unwrap().contains_key(&original_backend_id));

    let error = client
        .error("session.send", json!({ "id": fork, "text": "go on" }))
        .await;
    let configs = stats.started(3).await;
    assert_eq!(
        configs[2].fork_of.as_ref().map(|f| f.original.as_str()),
        Some(original_backend_id.as_str())
    );
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert!(error["data"].get("reason").is_none(), "{error:#}");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("the backend could not be started")
            && message.contains("the fake has no record"),
        "the backend's reason, got {message:?}"
    );
    assert_eq!(session_row(&mut client, &fork).await["state"], "error");
}

/// While a recordless fork's backend starts, copying its original again, a
/// message to the original is held off, as it is while `session.fork`'s
/// fork starts, so no turn begins that the copy would take part way.
// verifies: FLOW-RESUME-SESSION-R10
#[tokio::test(flavor = "multi_thread")]
async fn sends_to_the_original_wait_until_a_resumed_recordless_fork_is_ready() {
    let records = Records::default();
    let fake = FakeAdapter::quiet_restart(Arc::clone(&records));
    let start_gate = Arc::clone(&fake.start_gate);
    let (core, stats) = start_fake(fake).await;
    let mut client = core.client().await;
    let (original, fork, _, _) = recordless_fork(&mut client, &core, &records, &stats).await;
    // The original running, so a send to it that were let through would
    // reach its backend at once.
    client
        .result("session.send", json!({ "id": original, "text": "again" }))
        .await;
    assert_eq!(stats.messages.lock().unwrap().len(), 2);

    let held = start_gate.lock().await;
    client.result("session.resume", json!({ "id": fork })).await;
    let configs = stats.started(4).await;
    assert!(configs[3].fork_of.is_some(), "forked again");
    let error = client
        .error(
            "session.send",
            json!({ "id": original, "text": "meanwhile" }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert_eq!(stats.messages.lock().unwrap().len(), 2, "nothing more");

    drop(held);
    wait_for_state(&mut client, &fork, "idle").await;
    client
        .result("session.send", json!({ "id": original, "text": "now" }))
        .await;
}

/// Stopping a message that waits for a stopped session's backend to start
/// stops the message, not the session: the backend, started once, goes on
/// to be ready and idle.
// verifies: PROTO-CORE-API-R86
#[tokio::test(flavor = "multi_thread")]
async fn interrupting_a_send_that_waits_for_a_resume_leaves_the_session_idle() {
    let fake = FakeAdapter::quiet_restart(Records::default());
    let start_gate = Arc::clone(&fake.start_gate);
    let (core, stats) = start_fake(fake).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    let stops = stats.stops.load(Ordering::SeqCst);
    client.clear_notifications();

    let held = start_gate.lock().await;
    let send = client
        .request("session.send", json!({ "id": id, "text": "again" }))
        .await;
    client
        .notification_where(|n| {
            n["method"] == "session.send_progress" && n["params"]["phase"] == "starting"
        })
        .await;
    client
        .result("session.interrupt", json!({ "id": id }))
        .await;
    let response = client.response(send).await;
    assert_eq!(kind(&response["error"]), "invalid_state", "{response:#}");
    assert_eq!(response["error"]["data"]["reason"], "send_stopped");

    drop(held);
    wait_for_state(&mut client, &id, "idle").await;
    assert_eq!(stats.starts.load(Ordering::SeqCst), 2, "started once more");
    assert_eq!(stats.stops.load(Ordering::SeqCst), stops, "never stopped");
    let texts: Vec<String> = stats
        .messages
        .lock()
        .unwrap()
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert_eq!(texts, ["hello"], "the stopped message never sent");
    // Resumed: the next message goes to the backend running now.
    client
        .result("session.send", json!({ "id": id, "text": "after" }))
        .await;
    assert_eq!(stats.starts.load(Ordering::SeqCst), 2);
}

/// Why a start failed is the answer for that start only. Once a later
/// start succeeds it is forgotten: a message that then waits for a start
/// the user stops is told the session is not running, not the old reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_start_that_succeeds_forgets_why_an_earlier_one_failed() {
    let fake = FakeAdapter::quiet_restart(Records::default());
    let (fail_start, start_gate) = (Arc::clone(&fake.fail_start), Arc::clone(&fake.start_gate));
    let (core, stats) = start_fake(fake).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;

    // The backend does not start: the message is answered with its words.
    fail_start.store(true, Ordering::SeqCst);
    let error = client
        .error("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("the fake refused to start"), "{error:#}");

    // It starts, and is stopped.
    fail_start.store(false, Ordering::SeqCst);
    client
        .result("session.send", json!({ "id": id, "text": "once more" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    assert_eq!(stats.starts.load(Ordering::SeqCst), 3);
    client.clear_notifications();

    // A message waits for the next start, which the user stops before the
    // backend is ready. That start did not fail.
    let held = start_gate.lock().await;
    let send = client
        .request("session.send", json!({ "id": id, "text": "last" }))
        .await;
    client
        .notification_where(|n| {
            n["method"] == "session.send_progress" && n["params"]["phase"] == "starting"
        })
        .await;
    let stop = client.request("session.stop", json!({ "id": id })).await;
    let response = client.response(send).await;
    drop(held);
    client.response(stop).await;
    let error = &response["error"];
    assert_eq!(kind(error), "invalid_state", "{response:#}");
    assert_eq!(
        error["data"]["reason"], "session_not_running",
        "{response:#}"
    );
    let message = error["message"].as_str().unwrap();
    assert!(!message.contains("refused to start"), "{response:#}");
}

/// A resumed session is `starting` until its backend answers Core's first
/// request, then `idle`, though the backend reports no turn.
// verifies: PROTO-AGENT-EVENTS-R25
#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_session_goes_from_starting_to_idle_once_its_backend_answers() {
    let fake = FakeAdapter::quiet_restart(Records::default());
    let start_gate = Arc::clone(&fake.start_gate);
    let (core, stats) = start_fake(fake).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.ended").await;

    let held = start_gate.lock().await;
    client.result("session.resume", json!({ "id": id })).await;
    let starting = client.events_until("session.state").await;
    assert_eq!(starting.last().unwrap()["event"]["state"], "starting");
    // The backend has been asked to start, and has not answered.
    stats.started(2).await;
    assert_eq!(session_row(&mut client, &id).await["state"], "starting");

    drop(held);
    let ready = client.events_until("session.state").await;
    assert_eq!(
        ready.last().unwrap()["event"]["state"],
        "idle",
        "{ready:#?}"
    );
    assert_eq!(session_row(&mut client, &id).await["state"], "idle");
}

/// A session resumed while nobody reads its view is read afresh by the
/// next reader: the record's events, then the new run's.
// verifies: FLOW-RESUME-SESSION-R01
#[tokio::test(flavor = "multi_thread")]
async fn a_session_resumed_unread_is_read_from_its_record_then_live() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;

    client.result("session.resume", json!({ "id": id })).await;
    client
        .result("session.send", json!({ "id": id, "text": "again" }))
        .await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("assistant.text").await;
    let events = if messages(&events).len() < 4 {
        let mut all = events;
        all.extend(client.events_until("assistant.text").await);
        all
    } else {
        events
    };
    let seqs: Vec<u64> = events.iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (0..events.len() as u64).collect::<Vec<_>>());
    assert_eq!(
        messages(&events),
        [
            ("record".into(), "hello".into()),
            ("record".into(), "echo: hello".into()),
            ("live".into(), "again".into()),
            ("live".into(), "echo: again".into()),
        ]
    );
}

/// A message with attachments to a session whose folder is gone is refused
/// before anything is copied or the backend is said to be starting.
// verifies: FLOW-RESUME-SESSION-R23
#[tokio::test(flavor = "multi_thread")]
async fn a_send_with_attachments_to_a_session_whose_folder_is_gone_copies_nothing() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = stopped_session(&core, &mut client).await;
    let folder = stats.configs.lock().unwrap()[0].cwd.clone();
    std::fs::remove_dir_all(&folder).unwrap();
    let notes = core.dir.path().join("notes.txt");
    std::fs::write(&notes, "notes").unwrap();
    client.clear_notifications();

    let error = client
        .error(
            "session.send",
            json!({
                "id": id,
                "text": "again",
                "attachments": [{ "kind": "file", "path": notes.display().to_string() }],
            }),
        )
        .await;
    assert_eq!(error["data"]["reason"], "folder_gone", "{error:#}");
    // Nothing made the folder again by copying into it.
    assert!(!std::path::Path::new(&folder).exists());
    assert_eq!(stats.starts.load(Ordering::SeqCst), 1);
    // And no progress said the backend was starting.
    while let Ok(Some(n)) =
        tokio::time::timeout(std::time::Duration::from_millis(200), client.notification()).await
    {
        assert_ne!(n["params"]["phase"], "starting", "{n:#}");
    }
}

/// A backend that exits by itself is announced ended before Core has let go
/// of its run; a message sent in that moment resumes the session rather
/// than finding it still running.
// verifies: FLOW-RESUME-SESSION-R02
#[tokio::test(flavor = "multi_thread")]
async fn a_send_just_after_the_backend_exited_resumes_the_session() {
    let fake = FakeAdapter::new(1);
    let stats = Arc::clone(&fake.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(fake)];
    let core = start_with_adapters(
        tempdir(),
        |config| config.wind_down_pause = std::time::Duration::from_secs(2),
        adapters,
    )
    .await;
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
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "exit" }))
        .await;
    client.events_until("session.ended").await;

    client
        .result("session.send", json!({ "id": id, "text": "again" }))
        .await;
    let configs = stats.started(2).await;
    assert!(configs[1].resume.is_some(), "{:?}", configs[1]);
}

/// The same for `session.resume`: asked for once the session was announced
/// ended and before Core has let go of its run, it waits for that and
/// starts the backend again rather than finding the session still running.
// verifies: FLOW-RESUME-SESSION-R02
#[tokio::test(flavor = "multi_thread")]
async fn a_resume_just_after_the_backend_exited_starts_the_session_again() {
    let fake = FakeAdapter::new(1);
    let stats = Arc::clone(&fake.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(fake)];
    let core = start_with_adapters(
        tempdir(),
        |config| config.wind_down_pause = std::time::Duration::from_secs(2),
        adapters,
    )
    .await;
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
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "exit" }))
        .await;
    client.events_until("session.ended").await;

    client.result("session.resume", json!({ "id": id })).await;
    let configs = stats.started(2).await;
    assert!(configs[1].resume.is_some(), "{:?}", configs[1]);
}

/// A backend that names a session itself, as the fake does: Core holds no id
/// for it until the backend names it on its stream, and lists none until
/// then; from then on the list carries that id, also once the session stops.
// verifies: FLOW-RESUME-SESSION-R17
// verifies: FLOW-RESUME-SESSION-R20
#[tokio::test(flavor = "multi_thread")]
async fn the_list_carries_a_self_named_session_id_from_when_the_backend_names_it() {
    let records = Records::default();
    let fake = FakeAdapter::with_records(Arc::clone(&records));
    let start_gate = Arc::clone(&fake.start_gate);
    let (core, stats) = start_fake(fake).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;

    // The fake, unlike Claude Code, names a session itself as it starts:
    // while its start is held, it has reported no id.
    let held = start_gate.lock().await;
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    stats.started(1).await;
    let before = session_row(&mut client, &id).await;
    assert!(before.get("backend_session_id").is_none(), "{before:#}");

    // Once it has started, it has named the session on its stream.
    drop(held);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let after = loop {
        let row = session_row(&mut client, &id).await;
        if row.get("backend_session_id").is_some() {
            break row;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no id listed: {row:#}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    let backend_id = only_record(&records);
    assert_eq!(after["backend_session_id"], json!(backend_id));
    // A tile carries it as the list does, so a window that already has the
    // session learns it (FLOW-RESUME-SESSION-R20).
    let carrying = |n: &Value| {
        n["method"] == "session.tile"
            && n["params"]["session_id"] == id
            && n["params"]["backend_session_id"] == backend_id
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.notification_where(carrying),
    )
    .await
    .expect("no session.tile carried the backend's id");

    client.result("session.stop", json!({ "id": id })).await;
    let stopped = session_row(&mut client, &id).await;
    assert_eq!(stopped["backend_session_id"], json!(backend_id));
}

/// When the backend names a session anew on its stream, and nothing else
/// about the tile changes, Core still sends a tile carrying the id it now
/// carries, so a window that has the session learns it.
// verifies: FLOW-RESUME-SESSION-R20
#[tokio::test(flavor = "multi_thread")]
async fn a_tile_goes_out_when_the_backend_names_the_session_anew() {
    let (core, _stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while session_row(&mut client, &id)
        .await
        .get("backend_session_id")
        .is_none()
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fake never named the session"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "name as fake-renamed" }),
        )
        .await;
    let carrying = |n: &Value| {
        n["method"] == "session.tile"
            && n["params"]["session_id"] == id
            && n["params"]["backend_session_id"] == "fake-renamed"
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.notification_where(carrying),
    )
    .await
    .expect("no session.tile carried the new id");
}
