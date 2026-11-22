//! `session.fork` over the socket (PROTO-CORE-API, Sessions;
//! FLOW-FORK-SESSION), with the fake backend's record standing in for
//! Claude Code's (support/fake.rs).

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{ConversationEnd, OpenElsewhere, SessionConfig};
use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter};
use support::{Client, TestCore, git, kind, project, start_with_adapters, tempdir, workspace};

async fn start(fake: &Arc<FakeAdapter>) -> (TestCore, Client) {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(fake) as Arc<dyn Adapter>];
    // Detected, so the fake's models are offered and a session can name one.
    let core = start_with_adapters(tempdir(), support::detecting, adapters).await;
    let client = core.client().await;
    (core, client)
}

async fn create(core: &TestCore, client: &mut Client, extra: Value) -> Value {
    let workspace_id = workspace(client, core.dir.path()).await;
    let mut params = json!({ "workspace_id": workspace_id, "backend": fake::BACKEND });
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    client.result("session.create", params).await
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

async fn wait_for_state(client: &mut Client, id: &str, state: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while row(client, id).await["state"] != state {
        assert!(tokio::time::Instant::now() < deadline, "never {state}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A started session whose backend has named it, with a turn behind it.
async fn idle(core: &TestCore, client: &mut Client, extra: Value) -> String {
    let id = create(core, client, extra).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    settle(client, &id).await;
    id
}

/// Waits for a created session's backend, then takes a turn in it.
async fn settle(client: &mut Client, id: &str) {
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
    wait_for_state(client, id, "idle").await;
}

/// Waits for the fork of a session of the fake's to start, then takes away
/// the record the fake made it: the CLI writes none before the first message.
async fn without_a_record_of_its_own(fake: &FakeAdapter, original_backend_id: &str) {
    fork_started(fake).await;
    without_a_record_of_its_own_now(fake, original_backend_id);
}

/// How the backend of the fork of a session was started, once it has been.
async fn fork_config(fake: &FakeAdapter) -> SessionConfig {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(config) = fake.stats.configs.lock().unwrap().get(1) {
            return config.clone();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The fake's id for the fork of the session whose id is `original`.
fn fork_backend_id(fake: &FakeAdapter, original: &str) -> String {
    let records = fake.records.lock().unwrap();
    records.keys().find(|k| *k != original).unwrap().clone()
}

/// Waits for the fake to have made a record for a fork, as it does when the
/// fork's backend starts.
async fn fork_started(fake: &FakeAdapter) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.records.lock().unwrap().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Waits for the fork to reach the gate the test holds: its registry check
/// or its read of the record is under way, so the original is marked as
/// being forked.
async fn at_gate(fake: &FakeAdapter) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.stats.at_gate.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never reached the gate"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Takes away every record but the original's.
fn without_a_record_of_its_own_now(fake: &FakeAdapter, original_backend_id: &str) {
    fake.records
        .lock()
        .unwrap()
        .retain(|id, _| id == original_backend_id);
}

/// Core started again over the same folder and records.
async fn restarted(core: TestCore, client: &mut Client, fake: &FakeAdapter) -> (TestCore, Client) {
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    let again: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(Arc::clone(
        &fake.records,
    )))];
    let core = start_with_adapters(dir, |_| {}, again).await;
    let client = core.client().await;
    (core, client)
}

/// The refusal of a fork asked for now.
async fn refused_now(client: &mut Client, id: &str) -> String {
    refusal_now(client, id).await["message"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The whole error refusing a fork asked for now.
async fn refusal_now(client: &mut Client, id: &str) -> Value {
    let error = client.error("session.fork", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    error
}

/// How long a fork answers that an earlier one has not finished starting.
const STILL_STARTING: Duration = Duration::from_secs(10);

/// The refusal of a fork, once any earlier fork has finished starting.
async fn refused(client: &mut Client, id: &str) -> String {
    refusal(client, id).await["message"]
        .as_str()
        .unwrap()
        .to_string()
}

/// The whole error refusing a fork, once any earlier fork has finished
/// starting.
async fn refusal(client: &mut Client, id: &str) -> Value {
    let deadline = tokio::time::Instant::now() + STILL_STARTING;
    loop {
        let error = refusal_now(client, id).await;
        let message = error["message"].as_str().unwrap();
        if !message.contains("has not finished starting") {
            return error;
        }
        assert!(tokio::time::Instant::now() < deadline, "{message}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A fork, once any earlier fork has finished starting.
async fn forked(client: &mut Client, id: &str) -> Value {
    let deadline = tokio::time::Instant::now() + STILL_STARTING;
    loop {
        let response = client.call("session.fork", json!({ "id": id })).await;
        if response.get("error").is_none() {
            return response["result"].clone();
        }
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("has not finished starting"),
            "{response:#}"
        );
        assert!(tokio::time::Instant::now() < deadline, "{response:#}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// verifies: FLOW-FORK-SESSION-R01
// verifies: FLOW-FORK-SESSION-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_copies_the_originals_setup_and_conversation_and_leaves_it_as_it_was() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(
        &core,
        &mut client,
        json!({ "model": "model-b", "effort": "low", "permission_mode": "auto" }),
    )
    .await;
    let section = client
        .result("section.create", json!({ "name": "Work" }))
        .await;
    client
        .result(
            "session.arrange",
            json!({ "id": id, "pinned": true, "section_id": section["id"] }),
        )
        .await;
    client
        .result(
            "session.set_title",
            json!({ "id": id, "title": "Original" }),
        )
        .await;
    let before = row(&mut client, &id).await;
    let original_backend = fake.stats.configs.lock().unwrap().len();
    assert_eq!(original_backend, 1);
    let backend_id = fake.records.lock().unwrap().keys().next().unwrap().clone();
    fake.records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(fake::recorded_reply("reply-1", "the reply"));
    let record_before = fake.records.lock().unwrap().clone();

    let fork = client.result("session.fork", json!({ "id": id })).await;

    assert_ne!(fork["id"], before["id"]);
    // `workspace` makes an isolated workspace, so the fork gets its own.
    assert_ne!(fork["workspace_id"], before["workspace_id"]);
    assert_eq!(fork["backend"], before["backend"]);
    assert_eq!(fork["model"], "model-b");
    assert_eq!(fork["effort"], "low");
    assert_eq!(fork["permission_mode"], "auto");
    assert_eq!(fork["section_id"], section["id"]);
    assert_eq!(fork["pinned"], false);
    assert_eq!(fork["archived"], false);
    assert_eq!(fork["title"], Value::Null);

    let fork_id = fork["id"].as_str().unwrap().to_string();
    // The backend's session comes from a copy of the original's and reports
    // an id of its own.
    client
        .result("session.subscribe", json!({ "id": fork_id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.started").await;
    let started = events.last().unwrap();
    assert_ne!(started["event"]["backend_session_id"], backend_id.as_str());
    let config = fake.stats.configs.lock().unwrap()[1].clone();
    let fork_of = config.fork_of.clone().expect("a fork");
    assert_eq!(fork_of.original, backend_id);
    // verifies: FLOW-FORK-SESSION-R08
    assert_eq!(fork_of.name.as_deref(), Some("Original (fork)"));
    let first = fake.stats.configs.lock().unwrap()[0].clone();
    assert_ne!(config.cwd, first.cwd);
    assert_eq!(config.model.as_deref(), Some("model-b"));
    assert_eq!(config.effort.as_deref(), Some("low"));
    assert_eq!(config.permission_mode, "auto");

    // Its transcript holds the conversation copied, before any message.
    let copied: Vec<&Value> = events
        .iter()
        .filter(|e| e["source"]["input"] == "record" && e["event"]["type"] == "assistant.text")
        .collect();
    assert_eq!(copied.len(), 1, "{events:#?}");
    assert_eq!(copied[0]["event"]["text"], "the reply");

    // The original is unchanged: its record, its state, its arrangement.
    assert_eq!(
        fake.records.lock().unwrap()[&backend_id],
        record_before[&backend_id]
    );
    let after = row(&mut client, &id).await;
    for field in [
        "state",
        "title",
        "pinned",
        "section_id",
        "archived",
        "model",
    ] {
        assert_eq!(after[field], before[field], "{field}");
    }
}

// verifies: FLOW-FORK-SESSION-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_of_an_archived_session_is_not_archived() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;
    let fork = client.result("session.fork", json!({ "id": id })).await;
    assert_eq!(fork["archived"], false);
    assert_eq!(row(&mut client, &id).await["archived"], true);
}

// verifies: FLOW-FORK-SESSION-R03
// verifies: PROTO-CORE-API-R79
// verifies: PROTO-CORE-API-R62
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_is_refused_unless_the_record_ends_with_a_finished_reply() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let mut messages = Vec::new();
    for (end, says, reason) in [
        (
            Some(ConversationEnd::NoReply),
            "no reply has finished",
            "no_finished_reply",
        ),
        (
            Some(ConversationEnd::Interrupted),
            "was interrupted",
            "no_finished_reply",
        ),
        (
            Some(ConversationEnd::Unfinished),
            "part way through a turn",
            "no_finished_reply",
        ),
        (None, "cannot fork", "backend_cannot_fork"),
    ] {
        *fake.conversation_end.lock().unwrap() = end;
        let error = refusal(&mut client, &id).await;
        assert_eq!(error["data"]["reason"], reason, "{error:#}");
        let message = error["message"].as_str().unwrap().to_string();
        assert!(message.contains(says), "{message}");
        messages.push(message);
    }
    messages.dedup();
    assert_eq!(messages.len(), 4, "each case has its own message");
    assert_eq!(
        fake.stats.starts.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        client
            .result("session.list", json!({}))
            .await
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

// verifies: FLOW-FORK-SESSION-R03
// verifies: PROTO-CORE-API-R79
// verifies: PROTO-CORE-API-R62
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_is_refused_while_a_turn_runs() {
    for text in ["work", "ask"] {
        let fake = Arc::new(FakeAdapter::new(1));
        let (core, mut client) = start(&fake).await;
        let id = idle(&core, &mut client, json!({})).await;
        client
            .result("session.send", json!({ "id": id, "text": text }))
            .await;
        if text == "ask" {
            client.events_until("permission.requested").await;
        } else {
            wait_for_state(&mut client, &id, "running").await;
        }
        let error = refusal(&mut client, &id).await;
        assert_eq!(error["data"]["reason"], "session_busy", "{error:#}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("a turn is running"), "{text}: {message}");
    }
}

// verifies: FLOW-FORK-SESSION-R03
// verifies: PROTO-CORE-API-R79
// verifies: FLOW-FORK-SESSION-R03
// verifies: PROTO-CORE-API-R62
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_is_refused_while_a_message_is_on_its_way() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    // A message the backend is slow to take: no turn runs yet, and the one
    // it starts would be copied part way.
    let send = client
        .request(
            "session.send",
            json!({ "id": id, "text": "taken once interrupted" }),
        )
        .await;
    let mut other = core.client().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while row(&mut other, &id).await.get("sending") != Some(&json!("handing_over")) {
        assert!(tokio::time::Instant::now() < deadline, "never handed over");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let error = refusal_now(&mut other, &id).await;
    assert_eq!(error["data"]["reason"], "session_busy", "{error:#}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("a turn is running"), "{message}");

    // The backend takes the message; the send is answered.
    other.result("session.interrupt", json!({ "id": id })).await;
    let sent = client.response(send).await;
    assert!(sent.get("result").is_some(), "{sent:#}");
}

// verifies: PROTO-CORE-API-R62
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_is_refused_before_the_backend_has_named_the_session() {
    let fake = Arc::new(FakeAdapter::slow_start(Duration::from_secs(2)));
    let (core, mut client) = start(&fake).await;
    let id = create(&core, &mut client, json!({})).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let error = refusal(&mut client, &id).await;
    assert_eq!(error["data"]["reason"], "backend_unnamed", "{error:#}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("has not named it"), "{message}");
}

// verifies: FLOW-FORK-SESSION-R03
// verifies: PROTO-CORE-API-R79
// verifies: PROTO-CORE-API-R87
// verifies: PROTO-CORE-API-R62
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_counts_the_registrys_processes_against_cores_own() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;

    // Live: one process in the registry can be Core's own.
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    forked(&mut client, &id).await;

    // Two can't both be.
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Yes {
        pids: vec![4242, 4343],
    };
    let error = refusal(&mut client, &id).await;
    assert_eq!(error["data"]["reason"], "open_elsewhere", "{error:#}");
    // `detail` is the processes' ids alone, for the Interface to name.
    assert_eq!(error["data"]["detail"], "4242, 4343", "{error:#}");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("4242") && message.contains("4343"),
        "{message}"
    );

    // A registry that cannot say does not refuse.
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Unknown;
    forked(&mut client, &id).await;

    // Stopped: Core runs none, so any process is another's.
    client.result("session.stop", json!({ "id": id })).await;
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    let message = refused(&mut client, &id).await;
    assert!(message.contains("4242"), "{message}");
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::No;
    forked(&mut client, &id).await;
}

// verifies: FLOW-FORK-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_of_a_session_core_does_not_know_is_not_found() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (_core, mut client) = start(&fake).await;
    let error = client
        .error("session.fork", json!({ "id": "s-nope" }))
        .await;
    assert_eq!(kind(&error), "not_found", "{error:#}");
}

// verifies: FLOW-FORK-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_record_that_cannot_be_read_refuses_the_fork() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    fake.conversation_end_fails
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let message = refused(&mut client, &id).await;
    assert!(message.contains("could not be read"), "{message}");
}

// verifies: FLOW-FORK-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_message_sent_during_the_forks_reads_is_refused() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.open_check_gate);
    let held = gate.lock().await;
    let pending = client.request("session.fork", json!({ "id": id })).await;
    at_gate(&fake).await;

    let mut second = core.client().await;
    let error = second
        .error("session.send", json!({ "id": id, "text": "work" }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert!(
        error["message"].as_str().unwrap().contains("being forked"),
        "{error:#}"
    );

    drop(held);
    let response = client.response(pending).await;
    assert!(response["result"]["id"].is_string(), "{response}");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_one_fork_of_a_session_reads_its_record_at_a_time() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.open_check_gate);
    let held = gate.lock().await;
    let pending = client.request("session.fork", json!({ "id": id })).await;
    at_gate(&fake).await;

    let mut second = core.client().await;
    let message = refused_now(&mut second, &id).await;
    assert!(message.contains("has not finished starting"), "{message}");

    drop(held);
    let response = client.response(pending).await;
    assert!(response["result"]["id"].is_string(), "{response}");
    // Finished, the next one may go.
    forked(&mut second, &id).await;
}

// verifies: FLOW-FORK-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_session_removed_during_the_forks_reads_is_not_forked() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.open_check_gate);
    let held = gate.lock().await;
    let pending = client.request("session.fork", json!({ "id": id })).await;
    at_gate(&fake).await;

    let mut second = core.client().await;
    second.result("session.stop", json!({ "id": id })).await;
    second.result("session.remove", json!({ "id": id })).await;

    drop(held);
    let response = client.response(pending).await;
    assert_eq!(kind(&response["error"]), "not_found", "{response}");
    assert_eq!(
        fake.stats.starts.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "no fork started"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_registry_check_that_hangs_holds_back_the_next_fork_until_it_ends() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.open_check_gate);
    let held = gate.lock().await;

    // The registry doesn't answer in time, which doesn't refuse the fork,
    client.result("session.fork", json!({ "id": id })).await;
    // but its check is still running, so another try starts no second one.
    let message = refused_now(&mut client, &id).await;
    assert!(message.contains("has not finished starting"), "{message}");

    drop(held);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let response = client.call("session.fork", json!({ "id": id })).await;
        if response["result"]["id"].is_string() {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{response}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_of_the_record_that_hangs_holds_back_the_next_fork() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.read_gate);
    let held = gate.lock().await;
    let pending = client.request("session.fork", json!({ "id": id })).await;
    at_gate(&fake).await;

    let mut second = core.client().await;
    let message = refused_now(&mut second, &id).await;
    assert!(message.contains("has not finished starting"), "{message}");

    drop(held);
    let response = client.response(pending).await;
    assert!(response["result"]["id"].is_string(), "{response}");
}

// verifies: FLOW-FORK-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn the_original_takes_no_message_until_its_forks_backend_is_ready() {
    let fake = Arc::new(FakeAdapter::slow_start(Duration::from_millis(500)));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;

    let fork = client.result("session.fork", json!({ "id": id })).await;
    let error = client
        .error("session.send", json!({ "id": id, "text": "work" }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert!(
        error["message"].as_str().unwrap().contains("being forked"),
        "{error:#}"
    );

    // Let go once the fork's backend is ready.
    assert!(fork["id"].is_string());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let response = client
            .call("session.send", json!({ "id": id, "text": "idle" }))
            .await;
        if response.get("error").is_none() {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline, "{response:#}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// verifies: FLOW-FORK-SESSION-R01
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_never_sent_a_message_still_shows_its_conversation_after_a_restart() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let backend_id = fake.records.lock().unwrap().keys().next().unwrap().clone();
    let keep = |text: &str, reply: &str| {
        fake.records
            .lock()
            .unwrap()
            .get_mut(&backend_id)
            .unwrap()
            .push(fake::recorded_reply(text, reply));
    };
    keep("reply-1", "the reply");
    client
        .result("session.set_title", json!({ "id": id, "title": "Titled" }))
        .await;
    fake.records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(fake::recorded_title("Titled"));
    let section = client
        .result("section.create", json!({ "name": "Work" }))
        .await;
    client
        .result(
            "session.arrange",
            json!({ "id": id, "section_id": section["id"] }),
        )
        .await;
    let fork = client.result("session.fork", json!({ "id": id })).await;
    let fork_id = fork["id"].as_str().unwrap().to_string();
    without_a_record_of_its_own(&fake, &backend_id).await;
    // The original goes on without it, under a new name.
    keep("reply-2", "after the fork");
    fake.records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(fake::recorded_title("Later"));

    let (_core, mut client) = restarted(core, &mut client, &fake).await;

    client
        .result("session.subscribe", json!({ "id": fork_id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    // verifies: FLOW-FORK-SESSION-R08
    // Its name was the backend's, which keeps nothing of it yet.
    assert_eq!(row(&mut client, &fork_id).await["title"], Value::Null);
    let shown: Vec<&Value> = events
        .iter()
        .filter(|e| e["event"]["type"] == "assistant.text")
        .collect();
    assert_eq!(shown.len(), 1, "{events:#?}");
    assert_eq!(shown[0]["event"]["text"], "the reply");
    // verifies: FLOW-FORK-SESSION-R05
    assert_eq!(
        row(&mut client, &fork_id).await["section_id"],
        section["id"]
    );
}

// verifies: FLOW-FORK-SESSION-R09
#[tokio::test(flavor = "multi_thread")]
async fn a_forks_link_to_its_original_goes_once_it_has_a_record() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let original = fake.records.lock().unwrap().keys().next().unwrap().clone();
    fake.records
        .lock()
        .unwrap()
        .get_mut(&original)
        .unwrap()
        .push(fake::recorded_reply("reply-1", "the original's reply"));
    let fork_id = forked(&mut client, &id).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let started = fork_config(&fake).await;
    assert!(started.fork_of.is_some());
    fork_started(&fake).await;

    // Seen with a record of its own, as Core starts.
    let (core, mut client) = restarted(core, &mut client, &fake).await;
    assert_eq!(row(&mut client, &fork_id).await["id"], fork_id.as_str());

    // With that record gone there is nothing left to fall back on.
    without_a_record_of_its_own_now(&fake, &original);
    let (_core, mut client) = restarted(core, &mut client, &fake).await;
    client
        .result("session.subscribe", json!({ "id": fork_id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    assert!(
        !events
            .iter()
            .any(|e| e["event"]["text"] == "the original's reply"),
        "{events:#?}"
    );
}

// verifies: FLOW-FORK-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_with_a_record_of_its_own_shows_its_title_and_date() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let original = fake.records.lock().unwrap().keys().next().unwrap().clone();
    client
        .result("session.set_title", json!({ "id": id, "title": "Orig" }))
        .await;
    let fork = forked(&mut client, &id).await;
    let fork_id = fork["id"].as_str().unwrap().to_string();
    assert_eq!(fork["title"], Value::Null);
    fork_started(&fake).await;
    // Its record starts with the entries it copied, which keep their times.
    let reply = fake::recorded_reply("reply-1", "the reply");
    let copied_at = reply.ts.clone().unwrap();
    assert_ne!(fork["created_at"], copied_at.as_str());
    for (key, record) in fake.records.lock().unwrap().iter_mut() {
        if *key != original {
            record.push(reply.clone());
            record.push(fake::recorded_title("From the backend"));
        }
    }

    let (_core, mut client) = restarted(core, &mut client, &fake).await;

    let listed = row(&mut client, &fork_id).await;
    assert_eq!(listed["title"], "From the backend");
    assert_eq!(listed["created_at"], copied_at.as_str());
}

/// A fork whose first message never came, its original since deleted.
// verifies: FLOW-FORK-SESSION-R01
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_whose_conversation_is_gone_says_so() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let backend_id = fake.records.lock().unwrap().keys().next().unwrap().clone();
    fake.records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(fake::recorded_reply("reply-1", "the reply"));
    let fork = client.result("session.fork", json!({ "id": id })).await;
    let fork_id = fork["id"].as_str().unwrap().to_string();
    fork_started(&fake).await;
    // Neither record is left: the fork's was never written, the original's
    // went with its delete.
    fake.records.lock().unwrap().clear();

    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    let again: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(Arc::clone(
        &fake.records,
    )))];
    let core = start_with_adapters(dir, |_| {}, again).await;
    let mut client = core.client().await;

    client
        .result("session.subscribe", json!({ "id": fork_id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    let said = events
        .iter()
        .find(|e| e["event"]["type"] == "error")
        .unwrap_or_else(|| panic!("no error in {events:#?}"));
    let message = said["event"]["message"].as_str().unwrap();
    assert!(message.contains("record is gone"), "{message}");
}

// verifies: FLOW-FORK-SESSION-R03
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_before_its_first_message_is_told_so_when_forked() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let fork = forked(&mut client, &id).await;
    // Its backend names it as it starts.
    client
        .result(
            "session.subscribe",
            json!({ "id": fork["id"], "from_seq": 0 }),
        )
        .await;
    client.events_until("session.started").await;
    // The fake reports itself running once started; a real CLI waits idle.
    client
        .result("session.send", json!({ "id": fork["id"], "text": "idle" }))
        .await;
    wait_for_state(&mut client, fork["id"].as_str().unwrap(), "idle").await;
    *fake.conversation_end.lock().unwrap() = Some(ConversationEnd::NoReply);
    let error = refusal(&mut client, fork["id"].as_str().unwrap()).await;
    assert_eq!(error["data"]["reason"], "no_finished_reply", "{error:#}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("no message of its own"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_that_outlives_its_request_still_holds_back_the_next_fork() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.read_gate);
    let held = gate.lock().await;

    // The request gives up on the read, which goes on.
    let message = refused_now(&mut client, &id).await;
    assert!(message.contains("could not be read within"), "{message}");
    let message = refused_now(&mut client, &id).await;
    assert!(message.contains("has not finished starting"), "{message}");

    drop(held);
    forked(&mut client, &id).await;
}

/// A git repository at `dir/repo` added as a project, with an isolated
/// workspace on a commit of its own; returns the project and the workspace.
async fn isolated_workspace(client: &mut Client, dir: &std::path::Path) -> (Value, Value) {
    let project = project(client, dir).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    (project, workspace)
}

/// A git repository at `dir/repo` added as a project; returns its root
/// workspace.
async fn root_workspace(client: &mut Client, dir: &std::path::Path) -> Value {
    let project = project(client, dir).await;
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == "root")
        .cloned()
        .expect("a root workspace")
}

/// A started, idle session in `workspace`.
async fn idle_in(client: &mut Client, workspace: &Value) -> String {
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    settle(client, &id).await;
    id
}

/// The commit a folder has checked out.
fn head_of(folder: &std::path::Path) -> String {
    git(folder, &["rev-parse", "HEAD"])
}

// verifies: FLOW-FORK-SESSION-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_of_an_isolated_session_gets_its_own_workspace_and_branch_at_its_commit() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, workspace) = isolated_workspace(&mut client, core.dir.path()).await;
    let folder = std::path::PathBuf::from(workspace["path"].as_str().unwrap());
    // A commit only the original's branch has, and an edit not committed.
    std::fs::write(folder.join("committed.txt"), "committed\n").unwrap();
    git(&folder, &["add", "committed.txt"]);
    git(
        &folder,
        &["commit", "--quiet", "-m", "On the original's branch"],
    );
    std::fs::write(folder.join("uncommitted.txt"), "uncommitted\n").unwrap();
    let id = idle_in(&mut client, &workspace).await;

    let fork = forked(&mut client, &id).await;

    assert_ne!(fork["workspace_id"], workspace["id"]);
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    let made = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == fork["workspace_id"])
        .cloned()
        .expect("the fork's workspace is listed");
    assert_eq!(made["kind"], "isolated");
    assert_eq!(made["state"], "ready");
    assert_ne!(made["vcs_ref"], workspace["vcs_ref"]);
    let made_folder = std::path::PathBuf::from(made["path"].as_str().unwrap());
    assert_eq!(head_of(&made_folder), head_of(&folder));
    assert!(made_folder.join("committed.txt").exists());
    assert!(!made_folder.join("uncommitted.txt").exists());
    assert!(folder.join("uncommitted.txt").exists());
    fork_config(&fake).await;
    assert_eq!(
        fake.stats.configs.lock().unwrap()[1].cwd,
        made["path"].as_str().unwrap()
    );
}

// verifies: FLOW-FORK-SESSION-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_that_cannot_be_made_leaves_no_fork() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, workspace) = isolated_workspace(&mut client, core.dir.path()).await;
    let id = idle_in(&mut client, &workspace).await;
    client.result("session.stop", json!({ "id": id })).await;
    // Its folder gone, nothing says what commit to start from.
    let folder = workspace["path"].as_str().unwrap();
    std::fs::remove_dir_all(folder).unwrap();

    let error = client.error("session.fork", json!({ "id": id })).await;

    assert_ne!(kind(&error), "invalid_state", "{error:#}");
    let sessions = client.result("session.list", json!({})).await;
    assert_eq!(sessions.as_array().unwrap().len(), 1);
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(workspaces.as_array().unwrap().len(), 2, "{workspaces:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_original_cannot_be_deleted_until_its_forks_backend_is_ready() {
    let fake = Arc::new(FakeAdapter::slow_start(Duration::from_millis(500)));
    let (core, mut client) = start(&fake).await;
    let root = root_workspace(&mut client, core.dir.path()).await;
    let id = idle_in(&mut client, &root).await;

    client.result("session.fork", json!({ "id": id })).await;
    let error = client.error("session.delete", json!({ "id": id })).await;

    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert!(
        error["message"].as_str().unwrap().contains("being forked"),
        "{error:#}"
    );
}

// verifies: FLOW-FORK-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_long_title_is_shortened_to_fit_with_its_mark() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let title = "t".repeat(200);
    client
        .result("session.set_title", json!({ "id": id, "title": title }))
        .await;

    let fork = forked(&mut client, &id).await;

    // Core holds none of its own: the name goes to the backend.
    assert_eq!(fork["title"], Value::Null);
    let name = fork_config(&fake).await.fork_of.unwrap().name.unwrap();
    assert_eq!(name.chars().count(), 200);
    assert_eq!(name, format!("{} (fork)", "t".repeat(193)));
}

// verifies: FLOW-FORK-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_its_backend_did_not_name_has_no_title_until_its_record_gives_one() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let original = fake.records.lock().unwrap().keys().next().unwrap().clone();
    client
        .result("session.set_title", json!({ "id": id, "title": "Orig" }))
        .await;

    // The fake's backend announces no name.
    let fork = forked(&mut client, &id).await;
    let fork_id = fork["id"].as_str().unwrap().to_string();
    assert_eq!(fork["title"], Value::Null);
    settle(&mut client, &fork_id).await;
    assert_eq!(row(&mut client, &fork_id).await["title"], Value::Null);

    // Its record gives the title the backend copied, seen as it ends.
    let backend_id = fork_backend_id(&fake, &original);
    fake.records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(fake::recorded_title("Orig"));
    client
        .result("session.stop", json!({ "id": fork_id }))
        .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while row(&mut client, &fork_id).await["title"] != "Orig" {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the record's title never showed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// verifies: FLOW-FORK-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_of_an_untitled_session_has_no_title() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    assert_eq!(row(&mut client, &id).await["title"], Value::Null);

    let fork = forked(&mut client, &id).await;

    assert_eq!(fork["title"], Value::Null);
}

// verifies: FLOW-FORK-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn an_untitled_originals_later_title_is_not_its_forks() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let backend_id = fake.records.lock().unwrap().keys().next().unwrap().clone();
    let fork = forked(&mut client, &id).await;
    let fork_id = fork["id"].as_str().unwrap().to_string();
    without_a_record_of_its_own(&fake, &backend_id).await;
    fake.records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(fake::recorded_title("Later"));

    let (_core, mut client) = restarted(core, &mut client, &fake).await;

    assert_eq!(row(&mut client, &fork_id).await["title"], Value::Null);
}

// verifies: FLOW-FORK-SESSION-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_of_a_root_session_shares_its_workspace() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let root = root_workspace(&mut client, core.dir.path()).await;
    let id = idle_in(&mut client, &root).await;

    let fork = forked(&mut client, &id).await;

    assert_eq!(fork["workspace_id"], root["id"]);
    let workspaces = client
        .result(
            "workspace.list",
            json!({ "project_id": root["project_id"] }),
        )
        .await;
    assert_eq!(workspaces.as_array().unwrap().len(), 1, "{workspaces:#}");
}

/// A fork whose session Core refuses to record once its workspace is made,
/// so its cleanup runs: Core began shutting down while git made the
/// workspace, and from then admission records no new session. Core's git
/// fails worktree removals as the test says.
struct RefusedFork {
    core: TestCore,
    repo: std::path::PathBuf,
    /// The original's workspace; the fork's is made beside it.
    original: Value,
    folders_before: Vec<std::path::PathBuf>,
    branches_before: String,
    store: ersa_core::persistence::Store,
    /// Where every `git worktree add` stops.
    adds: Arc<ersa_core::Gate>,
}

/// Lets every add go however the test ends, so no request stays parked.
struct LetAddsGo(Arc<ersa_core::Gate>);

impl Drop for LetAddsGo {
    fn drop(&mut self) {
        for _ in 0..16 {
            self.0.go();
        }
    }
}

/// [`RefusedFork`], its cleanup under way. With `held_turn`, another
/// `workspace.create` asked for the project's turn while the fork's add
/// held it, and once the fork's add is done takes the turn and keeps it,
/// its own add stopped, longer than the cleanup's wait. Every wait for a
/// turn is 10 seconds: the second create waits from before Core begins
/// shutting down until the fork's add is done, which must stay under it
/// however busy the machine is, and the cleanup gives up after it.
async fn refused_fork(removal: support::deleting::Removal, held_turn: bool) -> RefusedFork {
    use support::deleting::FailingGit;
    let fake = Arc::new(FakeAdapter::new(1));
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(&fake) as Arc<dyn Adapter>];
    let adds = Arc::new(ersa_core::Gate::default());
    let failing = Arc::new(FailingGit::new(removal, None).stopping_adds(Arc::clone(&adds)));
    let core = start_with_adapters(
        tempdir(),
        move |config| {
            support::detecting(config);
            config.command_transport = Some(ersa_core::CommandTransport(failing));
            config.worktree_turn_wait = Duration::from_secs(10);
        },
        adapters,
    )
    .await;
    let mut client = core.client().await;
    let project = project(&mut client, core.dir.path()).await;
    let repo = core.dir.path().join("repo");
    let create = json!({ "project_id": project["id"] });
    let made = client.request("workspace.create", create.clone()).await;
    reached(&adds).await;
    adds.go();
    let original = client.response(made).await["result"].clone();
    let id = idle_in(&mut client, &original).await;
    let folders_before = workspace_folders(&original);
    let branches_before = git(&repo, &["branch", "--list", "--format=%(refname)"]);
    let db = core.dir.path().join("data").join("ersa.db");
    let store = ersa_core::persistence::Store::open(&db).expect("open");

    let _fork = client.request("session.fork", json!({ "id": id })).await;
    reached(&adds).await;
    if held_turn {
        let mut holder = core.client().await;
        let _holding = holder.request("workspace.create", create).await;
        // Its row is stored as `creating` just before it asks for the turn.
        until(&store, "the second create to wait for its turn", |rows| {
            rows.iter().filter(|(_, state)| state == "creating").count() == 2
        })
        .await;
    }
    let mut other = core.client().await;
    let _shutdown = other.request("core.shutdown", json!({})).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while fake.stats.stops.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "Core never began to shut down"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    adds.go();
    RefusedFork {
        core,
        repo,
        original,
        folders_before,
        branches_before,
        store,
        adds,
    }
}

/// Waits, up to a minute, for a request to stop at `gate`.
async fn reached(gate: &ersa_core::Gate) {
    tokio::time::timeout(Duration::from_secs(60), gate.reached())
        .await
        .expect("a request reached the gate in time");
}

/// Every stored workspace's id and state, in the order they were made.
async fn workspace_rows(store: &ersa_core::persistence::Store) -> Vec<(String, String)> {
    store
        .call(|conn| {
            let mut rows = conn.prepare("SELECT id, state FROM workspaces ORDER BY rowid")?;
            let rows = rows
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .expect("read the workspaces")
}

/// Waits, up to a minute, until the stored workspaces satisfy `done`.
async fn until(
    store: &ersa_core::persistence::Store,
    what: &str,
    done: impl Fn(&[(String, String)]) -> bool,
) -> Vec<(String, String)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let rows = workspace_rows(store).await;
        if done(&rows) {
            return rows;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "waited in vain for {what}: {rows:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A fork that fails to start, with git's removal of its new workspace
/// failing after git dropped its record: the cleanup goes on to the folder,
/// as a delete does, and leaves no workspace, folder or branch.
// verifies: PROTO-CORE-API-R122
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_that_fails_to_start_leaves_no_workspace_folder_or_branch() {
    let fork = refused_fork(support::deleting::Removal::DropsRecordThenFails, false).await;
    let _let_go = LetAddsGo(Arc::clone(&fork.adds));

    // The fork's cleanup runs on: its workspace's folder, branch and row go.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let rows = workspace_rows(&fork.store).await.len();
        let folders = workspace_folders(&fork.original);
        let branches = git(&fork.repo, &["branch", "--list", "--format=%(refname)"]);
        if rows == 2 && folders == fork.folders_before && branches == fork.branches_before {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "left behind: {rows} workspaces, folders {folders:?}, branches {branches}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let worktrees = git(&fork.repo, &["worktree", "list", "--porcelain"]);
    assert_eq!(worktrees.matches("worktree ").count(), 2, "{worktrees}");
    drop(fork.core);
}

/// A fork's cleanup whose git removal fails with the worktree still listed
/// leaves the fork's workspace `error`, its folder still there, so no
/// session starts in it.
// verifies: PROTO-CORE-API-R128
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_cleanup_that_fails_leaves_the_workspace_error() {
    let fork = refused_fork(support::deleting::Removal::Fails, false).await;
    let _let_go = LetAddsGo(Arc::clone(&fork.adds));
    let original = fork.original["id"].as_str().unwrap().to_string();

    let rows = until(&fork.store, "the fork's workspace to be error", |rows| {
        rows.iter()
            .any(|(id, state)| id != &original && state == "error")
    })
    .await;
    assert_eq!(rows.len(), 3, "{rows:?}");
    let (forks, _) = rows.iter().find(|(_, state)| state == "error").unwrap();
    let folders = workspace_folders(&fork.original);
    assert!(folders.iter().any(|f| f.ends_with(forks)), "{folders:?}");
    drop(fork.core);
}

/// A fork's cleanup that never had its turn at the project's worktrees
/// (another change held it past the wait) ran no git: the fork's workspace
/// is left as it was, `ready`, with its folder.
// verifies: PROTO-CORE-API-R127
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_cleanup_refused_its_turn_leaves_the_workspace_as_it_was() {
    let log = support::log::Log::installed();
    let fork = refused_fork(support::deleting::Removal::DropsRecordThenFails, true).await;
    let _let_go = LetAddsGo(Arc::clone(&fork.adds));
    // The second create took the turn: its add stopped.
    reached(&fork.adds).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !log.warned(
        "could not remove a fork's workspace",
        "did not finish in time",
    ) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the cleanup never gave up its wait: {:#?}",
            log.warnings()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Root, original, the fork's, and the second create's, still `creating`.
    let rows = workspace_rows(&fork.store).await;
    assert_eq!(rows.len(), 4, "{rows:?}");
    let (forks, state) = &rows[2];
    assert_eq!(state, "ready", "{rows:?}");
    let folders = workspace_folders(&fork.original);
    assert!(folders.iter().any(|f| f.ends_with(forks)), "{folders:?}");
    drop(fork.core);
}

/// The folders beside `workspace`'s under the workspaces root.
fn workspace_folders(workspace: &Value) -> Vec<std::path::PathBuf> {
    let parent = std::path::Path::new(workspace["path"].as_str().unwrap())
        .parent()
        .unwrap();
    let mut folders: Vec<_> = std::fs::read_dir(parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    folders.sort();
    folders
}
