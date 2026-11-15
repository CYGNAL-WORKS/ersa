//! A session's model, effort and permission mode as `session.list` reports
//! them (PROTO-CORE-API-R71): the backend's newest report of each, from its
//! live stream, else its record's turns after Core's request, else that
//! request. The fake backend's record and stream stand in for Claude Code's
//! (support/fake.rs).

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{Recorded, RecordedSettings, RelaunchSettings};
use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, RecordedSettingsMap, Records};
use support::{Client, TestCore, start_with_adapters, tempdir, workspace};
use time::OffsetDateTime;

/// The record and its settings, which outlive any Core, as the backend's do.
#[derive(Default, Clone)]
struct Backend {
    records: Records,
    settings: RecordedSettingsMap,
}

impl Backend {
    fn adapter(&self) -> Arc<FakeAdapter> {
        Arc::new(FakeAdapter::with_record_settings(
            Arc::clone(&self.records),
            Arc::clone(&self.settings),
        ))
    }

    /// The record of the one session the backend has, saying each value
    /// was last used by a turn that began at `began`.
    fn record_says(&self, model: &str, effort: &str, mode: &str, began: Option<OffsetDateTime>) {
        let backend_id = self.records.lock().unwrap().keys().next().unwrap().clone();
        self.record_of_says(&backend_id, model, effort, mode, began);
    }

    /// [`Backend::record_says`], of the session the backend knows as
    /// `backend_id`.
    fn record_of_says(
        &self,
        backend_id: &str,
        model: &str,
        effort: &str,
        mode: &str,
        began: Option<OffsetDateTime>,
    ) {
        self.settings.lock().unwrap().insert(
            backend_id.to_string(),
            RelaunchSettings {
                latest: turn(model, effort, mode, began),
                first: RecordedSettings::default(),
            },
        );
    }

    /// The record of the one session the backend has, as a relaunch reads
    /// it: the latest values, and those of the first turn after Core's
    /// requests.
    fn record_relaunch(&self, latest: RecordedSettings, first: RecordedSettings) {
        let backend_id = self.records.lock().unwrap().keys().next().unwrap().clone();
        self.settings
            .lock()
            .unwrap()
            .insert(backend_id, RelaunchSettings { latest, first });
    }
}

/// A turn that began at `began` and ran with these values.
fn turn(model: &str, effort: &str, mode: &str, began: Option<OffsetDateTime>) -> RecordedSettings {
    let recorded = |value: &str| {
        Some(Recorded {
            value: value.into(),
            turn_began: began,
        })
    };
    RecordedSettings {
        model: recorded(model),
        effort: recorded(effort),
        permission_mode: recorded(mode),
    }
}

async fn start(backend: &Backend, dir: tempfile::TempDir) -> (TestCore, Arc<FakeAdapter>) {
    let fake = backend.adapter();
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(&fake) as Arc<dyn Adapter>];
    // Detected, so the fake's models are offered and a session can name one.
    (
        start_with_adapters(dir, support::detecting, adapters).await,
        fake,
    )
}

/// A Core started again over the same data and the same backend, as after
/// the app was closed. The old one stops without stopping its sessions.
async fn restart(core: TestCore, backend: &Backend) -> (TestCore, Client) {
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;
    let (core, _) = start(backend, dir).await;
    let client = core.client().await;
    (core, client)
}

/// A new session with `extra` as `session.create`'s choices, its backend
/// started; returns its id.
async fn started(core: &TestCore, client: &mut Client, extra: Value) -> String {
    let workspace_id = workspace(client, core.dir.path()).await;
    let mut params = json!({ "workspace_id": workspace_id, "backend": fake::BACKEND });
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let id = client.result("session.create", params).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    id
}

async fn row(client: &mut Client, id: &str) -> Value {
    let list = client.result("session.list", json!({})).await;
    list.as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
        .unwrap()
}

/// The session's `(model, effort, permission_mode)` as `session.list` has it.
async fn values(client: &mut Client, id: &str) -> (Value, Value, Value) {
    let row = row(client, id).await;
    (
        row["model"].clone(),
        row["effort"].clone(),
        row["permission_mode"].clone(),
    )
}

/// Waits until `session.list` reports `expected` for the session.
async fn wait_for_values(client: &mut Client, id: &str, expected: (Value, Value, Value)) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let now = values(client, id).await;
        if now == expected {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "listed {now:?}, never {expected:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn three(model: &str, effort: &str, mode: &str) -> (Value, Value, Value) {
    (json!(model), json!(effort), json!(mode))
}

// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_request_stands_until_a_turn_after_it_ran_across_a_restart() {
    let backend = Backend::default();
    let (core, fake) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let before = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let id = started(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;
    assert_eq!(
        values(&mut client, &id).await,
        three("model-b", "low", "auto")
    );
    let config = fake.stats.configs.lock().unwrap()[0].clone();
    assert_eq!(
        (
            config.model.as_deref(),
            config.effort.as_deref(),
            config.permission_mode.as_str()
        ),
        (Some("model-b"), Some("low"), "auto")
    );

    // The record's latest turn began before Core's request: the request is
    // newer, and a restarted Core still knows when it was made.
    backend.record_says("model-a", "high", "ask", Some(before));
    drop(client);
    let (core, mut client) = restart(core, &backend).await;
    assert_eq!(
        values(&mut client, &id).await,
        three("model-b", "low", "auto")
    );

    // A turn that began after the request ran with other values: the
    // record's are the backend's newest.
    backend.record_says("model-a", "high", "ask", Some(OffsetDateTime::now_utc()));
    drop(client);
    let (_core, mut client) = restart(core, &backend).await;
    assert_eq!(
        values(&mut client, &id).await,
        three("model-a", "high", "ask")
    );
}

// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_value_core_never_requested_is_the_records_from_any_turn() {
    let backend = Backend::default();
    let (core, fake) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let id = started(&core, &mut client, json!({})).await;
    // Nothing named: the model and effort are the backend's to choose and
    // are absent until it says; the permission mode is always stated to the
    // backend, so it is the backend's default, requested now.
    assert_eq!(
        values(&mut client, &id).await,
        (Value::Null, Value::Null, json!("ask"))
    );
    let config = fake.stats.configs.lock().unwrap()[0].clone();
    assert_eq!(
        (config.model, config.effort, config.permission_mode.as_str()),
        (None, None, "ask")
    );

    // A turn of unknown start counts for a value Core never requested, and
    // not for the permission mode it did.
    backend.record_says("model-b", "low", "auto", None);
    drop(client);
    let (_core, mut client) = restart(core, &backend).await;
    assert_eq!(
        values(&mut client, &id).await,
        three("model-b", "low", "ask")
    );
}

// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_live_session_lists_what_its_stream_reports_and_its_record_once_it_ends() {
    let backend = Backend::default();
    let (core, _) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let id = started(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;

    // The stream's report stands over Core's request at once; effort, which
    // the stream does not report, stays the request.
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "report model-a plan" }),
        )
        .await;
    wait_for_values(&mut client, &id, three("model-a", "low", "plan")).await;

    // Once the run ends, the record says what the session last ran with.
    backend.record_says("model-b", "high", "ask", Some(OffsetDateTime::now_utc()));
    client.result("session.stop", json!({ "id": id })).await;
    wait_for_values(&mut client, &id, three("model-b", "high", "ask")).await;
}

// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_starts_with_the_values_its_original_lists() {
    let backend = Backend::default();
    let (core, fake) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let before = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let id = started(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "report model-a ask" }),
        )
        .await;
    wait_for_values(&mut client, &id, three("model-a", "low", "ask")).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while row(&mut client, &id).await["state"] != "idle" {
        assert!(tokio::time::Instant::now() < deadline, "never idle");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let original = backend
        .records
        .lock()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();

    let fork = client.result("session.fork", json!({ "id": id })).await;

    assert_eq!(
        (
            fork["model"].clone(),
            fork["effort"].clone(),
            fork["permission_mode"].clone()
        ),
        three("model-a", "low", "ask")
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let config = loop {
        if let Some(config) = fake.stats.configs.lock().unwrap().get(1) {
            break config.clone();
        }
        assert!(tokio::time::Instant::now() < deadline, "never started");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(
        (
            config.model.as_deref(),
            config.effort.as_deref(),
            config.permission_mode.as_str()
        ),
        (Some("model-a"), Some("low"), "ask")
    );

    // The fork's record holds the original's turns, from before the fork:
    // the values the fork was started with are newer, and a restarted Core
    // still knows when it gave them.
    let fork_id = fork["id"].as_str().unwrap().to_string();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let fork_backend = loop {
        let found = (backend.records.lock().unwrap().keys())
            .find(|k| **k != original)
            .cloned();
        if let Some(found) = found {
            break found;
        }
        assert!(tokio::time::Instant::now() < deadline, "no fork record");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    backend.record_of_says(&fork_backend, "model-b", "high", "auto", Some(before));
    drop(client);
    let (_core, mut client) = restart(core, &backend).await;
    assert_eq!(
        values(&mut client, &fork_id).await,
        three("model-a", "low", "ask")
    );
}

/// Stops `core` and takes away the times of its sessions' requests, as a
/// Core from before it kept them would have left the store.
async fn without_request_times(core: TestCore) -> tempfile::TempDir {
    let db = core.data_dir().join("ersa.db");
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE sessions SET model_at = NULL, effort_at = NULL, permission_mode_at = NULL",
        [],
    )
    .unwrap();
    dir
}

// verifies: PROTO-CORE-API-R97
#[tokio::test(flavor = "multi_thread")]
async fn a_value_stored_without_its_time_yields_to_the_records_from_any_turn() {
    let backend = Backend::default();
    let (core, _) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let before = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let id = started(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;
    drop(client);

    // The record holds none: the stored values stand.
    let dir = without_request_times(core).await;
    let (core, _) = start(&backend, dir).await;
    let mut client = core.client().await;
    assert_eq!(
        values(&mut client, &id).await,
        three("model-b", "low", "auto")
    );

    // The record holds values from a turn that began before the stored
    // request was made: with no time to compare, the record's stand.
    backend.record_says("model-a", "high", "ask", Some(before));
    drop(client);
    let (_core, mut client) = restart(core, &backend).await;
    assert_eq!(
        values(&mut client, &id).await,
        three("model-a", "high", "ask")
    );
}

/// Stops the session, resumes it, and returns the values its backend was
/// started with again: `(model, effort, permission_mode)`.
async fn relaunched(
    client: &mut Client,
    fake: &FakeAdapter,
    id: &str,
) -> (Option<String>, Option<String>, String) {
    client.result("session.stop", json!({ "id": id })).await;
    let starts = fake.stats.configs.lock().unwrap().len();
    client.result("session.resume", json!({ "id": id })).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let config = loop {
        if let Some(config) = fake.stats.configs.lock().unwrap().get(starts) {
            break config.clone();
        }
        assert!(tokio::time::Instant::now() < deadline, "never resumed");
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(config.resume.is_some(), "{config:?}");
    (config.model, config.effort, config.permission_mode)
}

fn passed(model: &str, effort: &str, mode: &str) -> (Option<String>, Option<String>, String) {
    (Some(model.into()), Some(effort.into()), mode.into())
}

/// No turn has begun since Core's requests: the relaunch passes them.
// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_relaunch_with_no_turn_after_the_request_passes_the_request() {
    let backend = Backend::default();
    let (core, fake) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let before = OffsetDateTime::now_utc() - time::Duration::hours(1);
    let id = started(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;
    backend.record_relaunch(
        turn("model-a", "high", "plan", Some(before)),
        RecordedSettings::default(),
    );
    assert_eq!(
        relaunched(&mut client, &fake, &id).await,
        passed("model-b", "low", "auto")
    );
}

/// The turns since Core's requests ran with what the first of them did, as
/// the backend names it: the relaunch passes the request, so an alias the
/// backend resolved stays an alias.
// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_relaunch_after_turns_that_kept_the_requested_values_passes_the_request() {
    let backend = Backend::default();
    let (core, fake) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let id = started(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;
    let first = OffsetDateTime::now_utc() + time::Duration::minutes(1);
    let later = first + time::Duration::minutes(1);
    backend.record_relaunch(
        turn("model-b-1", "low", "auto", Some(later)),
        turn("model-b-1", "low", "auto", Some(first)),
    );
    assert_eq!(
        relaunched(&mut client, &fake, &id).await,
        passed("model-b", "low", "auto")
    );
}

/// A later turn ran with values other than the first after Core's requests,
/// as when the user changed them in the backend's own tools: the relaunch
/// passes the record's latest for each value that changed, and the request
/// for one that did not.
// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_relaunch_after_a_later_turn_changed_a_value_passes_the_records_latest() {
    let backend = Backend::default();
    let (core, fake) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let id = started(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;
    let first = OffsetDateTime::now_utc() + time::Duration::minutes(1);
    let later = first + time::Duration::minutes(1);
    backend.record_relaunch(
        turn("model-a", "low", "plan", Some(later)),
        turn("model-b-1", "low", "auto", Some(first)),
    );
    assert_eq!(
        relaunched(&mut client, &fake, &id).await,
        passed("model-a", "low", "plan")
    );
}

/// A value Core never gave the backend has no request to weigh: the
/// relaunch passes the record's latest from any turn, as `session.list`
/// reports it.
// verifies: PROTO-CORE-API-R71
#[tokio::test(flavor = "multi_thread")]
async fn a_relaunch_passes_the_records_latest_for_a_value_core_never_requested() {
    let backend = Backend::default();
    let (core, fake) = start(&backend, tempdir()).await;
    let mut client = core.client().await;
    let id = started(&core, &mut client, json!({ "permission_mode": "auto" })).await;
    let before = OffsetDateTime::now_utc() - time::Duration::hours(1);
    backend.record_relaunch(
        turn("model-a", "high", "plan", Some(before)),
        RecordedSettings::default(),
    );
    assert_eq!(
        relaunched(&mut client, &fake, &id).await,
        passed("model-a", "high", "auto")
    );
}
