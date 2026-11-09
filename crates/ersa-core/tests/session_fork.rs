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
use support::{Client, TestCore, git, kind, project, start_with_adapters, tempdir, workspace};

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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.records.lock().unwrap().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    fake.records
        .lock()
        .unwrap()
        .retain(|id, _| id == original_backend_id);
}

/// Waits for the fake to have been sent `count` renames.
async fn renames_reach(fake: &FakeAdapter, count: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.stats.rename_titles.lock().unwrap().len() < count {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork was not named"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
    let error = client.error("session.fork", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    error["message"].as_str().unwrap().to_string()
}

/// How long a fork answers that an earlier one has not finished starting.
const STILL_STARTING: Duration = Duration::from_secs(10);

/// The refusal of a fork, once any earlier fork has finished starting.
async fn refused(client: &mut Client, id: &str) -> String {
    let deadline = tokio::time::Instant::now() + STILL_STARTING;
    loop {
        let message = refused_now(client, id).await;
        if !message.contains("has not finished starting") {
            return message;
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
    assert_eq!(fork["title"], "Original (fork)");

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
    assert_eq!(config.fork_of.as_deref(), Some(backend_id.as_str()));
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
    forked(&mut client, &id).await;

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
    tokio::time::sleep(Duration::from_millis(200)).await;

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
    tokio::time::sleep(Duration::from_millis(200)).await;

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
    tokio::time::sleep(Duration::from_millis(200)).await;

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

/// A fork whose first message never came, its original since deleted.
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
    assert_eq!(fork["title"], "Orig (fork)");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.records.lock().unwrap().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.records.lock().unwrap().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
    let message = refused(&mut client, fork["id"].as_str().unwrap()).await;
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.stats.configs.lock().unwrap().len() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the fork never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
    let fake = Arc::new(FakeAdapter::with_chosen_ids());
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    let title = "t".repeat(200);
    client
        .result("session.set_title", json!({ "id": id, "title": title }))
        .await;

    let fork = forked(&mut client, &id).await;

    let got = fork["title"].as_str().unwrap();
    assert_eq!(got.chars().count(), 200);
    assert_eq!(got, format!("{} (fork)", "t".repeat(193)));
    // The same name goes to the backend.
    renames_reach(&fake, 2).await;
    assert_eq!(fake.stats.rename_titles.lock().unwrap()[1], got);
}

// verifies: FLOW-FORK-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_name_leaves_the_mark_until_the_forks_record_gives_the_copied_title() {
    let fake = Arc::new(FakeAdapter::with_chosen_ids());
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    client
        .result("session.set_title", json!({ "id": id, "title": "Orig" }))
        .await;
    fake.stats
        .rename_refused
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let fork_id = forked(&mut client, &id).await["id"]
        .as_str()
        .unwrap()
        .to_string();

    renames_reach(&fake, 2).await;
    // The fork works, and is still marked.
    settle(&mut client, &fork_id).await;
    assert_eq!(row(&mut client, &fork_id).await["title"], "Orig (fork)");

    // Its record gives the title the backend copied, seen as it ends.
    let config = fake.stats.configs.lock().unwrap()[1].clone();
    fake.records
        .lock()
        .unwrap()
        .get_mut(config.backend_session_id.as_deref().unwrap())
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

// verifies: FLOW-FORK-SESSION-R08
#[tokio::test(flavor = "multi_thread")]
async fn a_rename_while_a_fork_starts_comes_after_the_name_core_gives_it() {
    let fake = Arc::new(FakeAdapter::with_chosen_ids());
    let (core, mut client) = start(&fake).await;
    let id = idle(&core, &mut client, json!({})).await;
    client
        .result("session.set_title", json!({ "id": id, "title": "Orig" }))
        .await;
    let titles = || fake.stats.rename_titles.lock().unwrap().clone();

    let gate = fake.stats.rename_gate.lock().await;
    let fork_id = forked(&mut client, &id).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    renames_reach(&fake, 2).await;
    let mut renaming = core.client().await;
    let (asked, title) = (fork_id.clone(), json!("Mine"));
    let renamed = tokio::spawn(async move {
        renaming
            .result("session.set_title", json!({ "id": asked, "title": title }))
            .await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(titles(), ["Orig", "Orig (fork)"]);

    drop(gate);
    renamed.await.unwrap();

    assert_eq!(titles(), ["Orig", "Orig (fork)", "Mine"]);
    assert_eq!(row(&mut client, &fork_id).await["title"], "Mine");
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
