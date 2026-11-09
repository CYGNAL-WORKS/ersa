//! `session.fork` over the socket (PROTO-CORE-API, Sessions;
//! FLOW-FORK-SESSION), with the fake backend's record standing in for
//! Claude Code's (support/fake.rs).

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{ConversationEnd, OpenElsewhere};
use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter};
use support::{Client, TestCore, kind, start_with_adapters, tempdir, workspace};

async fn start(fake: &Arc<FakeAdapter>) -> (TestCore, Client) {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(fake) as Arc<dyn Adapter>];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
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
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
    wait_for_state(client, &id, "idle").await;
    id
}

async fn refused(client: &mut Client, id: &str) -> String {
    let error = client.error("session.fork", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    error["message"].as_str().unwrap().to_string()
}

// verifies: FLOW-FORK-SESSION-R01
// verifies: FLOW-FORK-SESSION-R02
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
    assert_eq!(fork["workspace_id"], before["workspace_id"]);
    assert_eq!(fork["backend"], before["backend"]);
    assert_eq!(fork["model"], "model-b");
    assert_eq!(fork["effort"], "low");
    assert_eq!(fork["permission_mode"], "auto");
    assert_eq!(fork["section_id"], section["id"]);
    assert_eq!(fork["pinned"], false);
    assert_eq!(fork["archived"], false);
    assert_eq!(fork["title"], "Original");

    let fork_id = fork["id"].as_str().unwrap().to_string();
    // The backend's session comes from a copy of the original's, in its
    // workspace, and reports an id of its own.
    client
        .result("session.subscribe", json!({ "id": fork_id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.started").await;
    let started = events.last().unwrap();
    assert_ne!(started["event"]["backend_session_id"], backend_id.as_str());
    let config = fake.stats.configs.lock().unwrap()[1].clone();
    assert_eq!(config.fork_of.as_deref(), Some(backend_id.as_str()));
    let first = fake.stats.configs.lock().unwrap()[0].clone();
    assert_eq!(config.cwd, first.cwd);
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

// verifies: FLOW-FORK-SESSION-R02
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
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_is_refused_unless_the_record_ends_with_a_finished_reply() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let mut messages = Vec::new();
    for (end, says) in [
        (Some(ConversationEnd::NoReply), "no reply has finished"),
        (Some(ConversationEnd::Interrupted), "was interrupted"),
        (Some(ConversationEnd::Unfinished), "part way through a turn"),
        (None, "cannot fork"),
    ] {
        *fake.conversation_end.lock().unwrap() = end;
        let message = refused(&mut client, &id).await;
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
        let message = refused(&mut client, &id).await;
        assert!(message.contains("a turn is running"), "{text}: {message}");
    }
}

// verifies: FLOW-FORK-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_is_refused_before_the_backend_has_named_the_session() {
    let fake = Arc::new(FakeAdapter::slow_start(Duration::from_secs(2)));
    let (core, mut client) = start(&fake).await;
    let id = create(&core, &mut client, json!({})).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let message = refused(&mut client, &id).await;
    assert!(message.contains("has not named it"), "{message}");
}

// verifies: FLOW-FORK-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_fork_counts_the_registrys_processes_against_cores_own() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;

    // Live: one process in the registry can be Core's own.
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    client.result("session.fork", json!({ "id": id })).await;

    // Two can't both be.
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Yes {
        pids: vec![4242, 4343],
    };
    let message = refused(&mut client, &id).await;
    assert!(
        message.contains("4242") && message.contains("4343"),
        "{message}"
    );

    // A registry that cannot say does not refuse.
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Unknown;
    client.result("session.fork", json!({ "id": id })).await;

    // Stopped: Core runs none, so any process is another's.
    client.result("session.stop", json!({ "id": id })).await;
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    let message = refused(&mut client, &id).await;
    assert!(message.contains("4242"), "{message}");
    *fake.open_elsewhere.lock().unwrap() = OpenElsewhere::No;
    client.result("session.fork", json!({ "id": id })).await;
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
async fn a_turn_that_starts_during_the_forks_reads_refuses_it() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.open_check_gate);
    let held = gate.lock().await;
    let pending = client.request("session.fork", json!({ "id": id })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut second = core.client().await;
    second
        .result("session.send", json!({ "id": id, "text": "work" }))
        .await;
    wait_for_state(&mut second, &id, "running").await;

    drop(held);
    let response = client.response(pending).await;
    assert_eq!(kind(&response["error"]), "invalid_state", "{response}");
    let message = response["error"]["message"].as_str().unwrap();
    assert!(message.contains("a turn is running"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn only_one_fork_of_a_session_reads_its_record_at_a_time() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let gate = Arc::clone(&fake.open_check_gate);
    let held = gate.lock().await;
    let pending = client.request("session.fork", json!({ "id": id })).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut second = core.client().await;
    let message = refused(&mut second, &id).await;
    assert!(message.contains("has not finished starting"), "{message}");

    drop(held);
    let response = client.response(pending).await;
    assert!(response["result"]["id"].is_string(), "{response}");
    // Finished, the next one may go.
    second.result("session.fork", json!({ "id": id })).await;
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
    tokio::time::sleep(Duration::from_millis(200)).await;

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
    let message = refused(&mut client, &id).await;
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
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut second = core.client().await;
    let message = refused(&mut second, &id).await;
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

    wait_for_state(&mut client, fork["id"].as_str().unwrap(), "idle").await;
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
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
    let fork = client.result("session.fork", json!({ "id": id })).await;
    let fork_id = fork["id"].as_str().unwrap().to_string();
    // The fork's backend wrote no record, as the CLI writes none yet.
    let own: Vec<String> = fake
        .records
        .lock()
        .unwrap()
        .keys()
        .filter(|k| **k != backend_id)
        .cloned()
        .collect();
    for key in own {
        fake.records.lock().unwrap().remove(&key);
    }
    // The original goes on without it.
    keep("reply-2", "after the fork");

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
    let events = client.events_until("assistant.text").await;
    let shown: Vec<&Value> = events
        .iter()
        .filter(|e| e["event"]["type"] == "assistant.text")
        .collect();
    assert_eq!(shown.len(), 1, "{events:#?}");
    assert_eq!(shown[0]["event"]["text"], "the reply");
}
