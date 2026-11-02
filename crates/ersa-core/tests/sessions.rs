//! The session manager driven through the Core API over the real socket,
//! with a fake adapter (`support/fake.rs`) standing in for a backend:
//! sequencing, subscriptions, tiles, the raw log, stopping, what a restarted
//! Core does with the sessions it finds, and opening a session from its
//! backend's record.

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

/// A new session of the fake backend in a new workspace; returns its id.
async fn new_session(core: &TestCore, client: &mut Client) -> String {
    let workspace_id = workspace(client, core.dir.path()).await;
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await;
    assert_eq!(session["state"], "starting");
    session["id"].as_str().unwrap().to_string()
}

fn seqs(events: &[Value]) -> Vec<u64> {
    events.iter().map(|e| e["seq"].as_u64().unwrap()).collect()
}

fn types(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .map(|e| e["event"]["type"].as_str().unwrap())
        .collect()
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

// verifies: PROTO-AGENT-EVENTS-R01
#[tokio::test(flavor = "multi_thread")]
async fn events_are_numbered_from_zero_without_gaps() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let sent = client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    assert_eq!(sent["turn"], 1);

    let events = client.events_until("assistant.text").await;
    assert_eq!(seqs(&events), (0..events.len() as u64).collect::<Vec<_>>());
    for event in &events {
        assert_eq!(event["v"], 2);
        assert_eq!(event["session_id"], id.as_str());
        assert_eq!(event["source"]["backend"], fake::BACKEND);
    }
    assert_eq!(
        types(&events),
        [
            "session.state",
            "backend.passthrough",
            "session.started",
            "session.state",
            "turn.started",
            "user.message",
            "usage.updated",
            "assistant.text"
        ]
    );
    let inputs: Vec<&str> = events
        .iter()
        .map(|e| e["source"]["input"].as_str().unwrap())
        .collect();
    assert_eq!(
        inputs,
        [
            "core", "live", "live", "live", "live", "live", "live", "live"
        ]
    );
    assert_eq!(events[0]["event"]["state"], "starting");
    assert_eq!(events[3]["event"]["state"], "running");
    // A live event's `ts` is when Core received its line.
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let received: Vec<&Value> = log.as_array().unwrap().iter().map(|l| &l["ts"]).collect();
    for event in &events[1..] {
        assert!(received.contains(&&event["ts"]), "{event:#}");
    }
    assert_eq!(events[7]["source"]["message_id"], "r-2");

    let second = client
        .result("session.send", json!({ "id": id, "text": "again" }))
        .await;
    assert_eq!(second["turn"], 2);
}

// verifies: PROTO-CORE-API-R14
#[tokio::test(flavor = "multi_thread")]
async fn subscribing_from_a_seq_replays_then_continues_live() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut first = core.client().await;
    let id = new_session(&core, &mut first).await;
    first
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    first
        .result("session.send", json!({ "id": id, "text": "one" }))
        .await;
    let before = first.events_until("assistant.text").await;
    let last = *seqs(&before).last().unwrap();

    let mut second = core.client().await;
    let subscribed = second
        .result("session.subscribe", json!({ "id": id, "from_seq": 3 }))
        .await;
    assert_eq!(subscribed["last_seq"], last);
    first
        .result("session.send", json!({ "id": id, "text": "two" }))
        .await;
    let mut got: Vec<Value> = Vec::new();
    while got.last().is_none_or(|e| e["event"]["text"] != "echo: two") {
        got.extend(second.events_until("assistant.text").await);
    }

    // From 3 onward, every event once, in order: the replayed part is what
    // the first client saw, and the live part carries straight on.
    assert_eq!(seqs(&got), (3..3 + got.len() as u64).collect::<Vec<_>>());
    assert_eq!(&got[..before.len() - 3], &before[3..]);
    assert_eq!(got.last().unwrap()["event"]["text"], "echo: two");

    // Subscribing again replaces the subscription rather than doubling it.
    second
        .result(
            "session.subscribe",
            json!({ "id": id, "from_seq": last + 1 }),
        )
        .await;
    first
        .result("session.send", json!({ "id": id, "text": "three" }))
        .await;
    let again = second.events_until("assistant.text").await;
    let unique: std::collections::BTreeSet<u64> = seqs(&again).into_iter().collect();
    assert_eq!(unique.len(), again.len(), "no event twice: {again:#?}");
    assert_eq!(seqs(&again)[0], last + 1);
}

// verifies: PROTO-CORE-API-R03
#[tokio::test(flavor = "multi_thread")]
async fn disconnecting_every_client_leaves_the_session_running() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let id = {
        let mut client = core.client().await;
        let id = new_session(&core, &mut client).await;
        client
            .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
            .await;
        client
            .result("session.send", json!({ "id": id, "text": "one" }))
            .await;
        client.events_until("assistant.text").await;
        id
        // The only client disconnects here.
    };

    let mut client = core.client().await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["state"], "running");
    assert_eq!(stats.stops.load(Ordering::SeqCst), 0);
    let sent = client
        .result("session.send", json!({ "id": id, "text": "two" }))
        .await;
    assert_eq!(sent["turn"], 2);

    // Reconnected, the whole stream replays without gaps or duplicates.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let first = client.events_until("assistant.text").await;
    let second = client.events_until("assistant.text").await;
    let all: Vec<Value> = first.into_iter().chain(second).collect();
    assert_eq!(seqs(&all), (0..all.len() as u64).collect::<Vec<_>>());
    assert_eq!(all.last().unwrap()["event"]["text"], "echo: two");
}

// verifies: PROTO-CORE-API-R04
// verifies: PROTO-CORE-API-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_permission_decision_reaches_the_backend_and_the_tile_follows() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;

    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    let tile = session_row(&mut client, &id).await["tile"].clone();
    assert_eq!(tile["context_tokens"], 50);
    assert_eq!(tile["context_window"], 200_000);
    assert_eq!(tile["cost_usd_total"], 0.05);

    client
        .result("session.send", json!({ "id": id, "text": "ask" }))
        .await;
    let asked = client.events_until("permission.requested").await;
    let request_id = asked.last().unwrap()["event"]["request_id"]
        .as_str()
        .unwrap()
        .to_string();
    let pending = json!([{ "kind": "permission", "request_id": request_id }]);
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["tile"]["pending"], pending);
    // The latest usage is kept while the request waits.
    assert_eq!(row["tile"]["context_tokens"], 50);
    // Every attached client hears about the tile without subscribing.
    let mut watcher = core.client().await;
    watcher
        .result("session.send", json!({ "id": id, "text": "hi" }))
        .await;
    let notified = watcher
        .notification_where(|n| n["method"] == "session.tile")
        .await;
    assert_eq!(notified["params"]["session_id"], id.as_str());
    assert_eq!(notified["params"]["tile"]["context_tokens"], 20);
    assert_eq!(notified["params"]["tile"]["pending"], pending);

    let decided = client
        .result(
            "session.decide_permission",
            json!({ "id": id, "request_id": request_id, "behavior": "allow" }),
        )
        .await;
    assert_eq!(decided["id"], id.as_str());
    let events = client.events_until("permission.decided").await;
    let event = &events.last().unwrap()["event"];
    assert_eq!(event["request_id"], request_id.as_str());
    assert_eq!(event["behavior"], "allow");
    assert_eq!(event["decided_by"], "user");
    let cleared = watcher
        .notification_where(|n| {
            n["method"] == "session.tile" && n["params"]["tile"]["pending"] == json!([])
        })
        .await;
    assert_eq!(cleared["params"]["tile"]["context_tokens"], 20);
    assert_eq!(
        session_row(&mut client, &id).await["tile"]["pending"],
        json!([])
    );

    // The request is answered; a second answer is refused.
    let again = client
        .error(
            "session.decide_permission",
            json!({ "id": id, "request_id": request_id, "behavior": "deny" }),
        )
        .await;
    assert_eq!(kind(&again), "not_found");
}

// verifies: PROTO-CORE-API-R05
#[tokio::test(flavor = "multi_thread")]
async fn a_question_waiting_on_the_user_is_in_the_tile() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "question" }))
        .await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(
        row["tile"]["pending"],
        json!([{ "kind": "question", "request_id": "q-1" }])
    );
    // A backend that has gone is waiting on nobody.
    client.result("session.stop", json!({ "id": id })).await;
    assert_eq!(
        session_row(&mut client, &id).await["tile"]["pending"],
        json!([])
    );
}

// verifies: PROTO-CORE-API-R08
// verifies: PROTO-AGENT-EVENTS-R13
#[tokio::test(flavor = "multi_thread")]
async fn the_raw_log_holds_every_line_exactly_as_it_crossed() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    // Read while the session is open: once nobody reads a session Core is
    // not running, its raw log goes.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "hello \"there\"" }),
        )
        .await;
    client.result("session.stop", json!({ "id": id })).await;

    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let log = log.as_array().unwrap();
    let lines: Vec<(&str, &str)> = log
        .iter()
        .map(|e| (e["dir"].as_str().unwrap(), e["line"].as_str().unwrap()))
        .collect();
    let user = json!({ "t": "user", "text": "hello \"there\"" }).to_string();
    let usage = json!({ "t": "usage", "tokens": 130 }).to_string();
    let reply = json!({ "t": "reply", "id": "r-2", "text": "echo: hello \"there\"" }).to_string();
    let adapter_lines: Vec<(&str, &str)> = lines
        .iter()
        .copied()
        .skip(2) // the start: hello and ready
        .collect();
    assert_eq!(
        adapter_lines,
        [
            ("host", user.as_str()),
            ("backend", usage.as_str()),
            ("backend", reply.as_str())
        ]
    );
    // Nothing but what crossed: Core's lifecycle facts are in the app log.
    assert_eq!(lines.len(), 5);
    let facts: Vec<String> = ersa_core::app_log::read_entries(&core.data_dir())
        .unwrap()
        .into_iter()
        .filter(|e| e.session_id == id)
        .map(|e| {
            serde_json::to_value(&e.fact).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(facts, ["process_started", "process_exited"]);
    let indices: Vec<u64> = log.iter().map(|e| e["index"].as_u64().unwrap()).collect();
    assert_eq!(indices, (0..log.len() as u64).collect::<Vec<_>>());

    let window = client
        .result("session.raw_log", json!({ "id": id, "from": 2, "to": 3 }))
        .await;
    assert_eq!(window.as_array().unwrap(), &log[2..=3]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscriber_that_falls_behind_is_disconnected_without_stalling_the_session() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut driver = core.client().await;
    let id = new_session(&core, &mut driver).await;
    let mut slow = core.client().await;
    slow.result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;

    // Far more events than the subscriber's queue and the pipe can hold,
    // while the slow client reads nothing.
    driver
        .result("session.send", json!({ "id": id, "text": "burst 20000" }))
        .await;
    let sent = driver
        .result("session.send", json!({ "id": id, "text": "after" }))
        .await;
    assert_eq!(sent["turn"], 2);

    // The slow client finds its connection closed after what it was sent.
    let mut read = 0;
    while slow.read_raw().await.is_some() {
        read += 1;
    }
    assert!(read < 20_000, "disconnected early, after {read} lines");
    // The session carried on for everyone else.
    driver
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let all = driver.events_until("user.message").await;
    assert_eq!(types(&all)[0], "session.state");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_session_has_ended_and_can_then_be_removed() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let workspace_id = session_row(&mut client, &id).await["workspace_id"].clone();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    // Ready, so the stop reaches the backend rather than cancelling its start.
    client.events_until("session.started").await;

    let stopped = client.result("session.stop", json!({ "id": id })).await;
    assert_eq!(stopped["state"], "ended");
    assert_eq!(stats.stops.load(Ordering::SeqCst), 1);
    let events = client.events_until("session.ended").await;
    let ended = &events.last().unwrap()["event"];
    assert_eq!(ended["reason"], "user_stopped");
    assert_eq!(ended["exit_code"], 0);
    // Stopping again changes nothing.
    client.result("session.stop", json!({ "id": id })).await;

    let error = client
        .error("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let error = client.error("session.start", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state");
    assert!(error["message"].as_str().unwrap().contains("M1"));

    // Removal never cascades: the workspace keeps its session until the
    // session is removed.
    let error = client
        .error("workspace.remove", json!({ "id": workspace_id }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let removed = client.result("session.remove", json!({ "id": id })).await;
    assert_eq!(removed["id"], id.as_str());
    assert_eq!(client.result("session.list", json!({})).await, json!([]));
    let error = client.error("session.raw_log", json!({ "id": id })).await;
    assert_eq!(kind(&error), "not_found");
    client
        .result("workspace.remove", json!({ "id": workspace_id }))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_session_cannot_be_removed_and_modes_change() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let error = client.error("session.remove", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state");
    let moded = client
        .result("session.set_mode", json!({ "id": id, "mode": "review" }))
        .await;
    assert_eq!(moded["mode"], "review");
    let error = client
        .error(
            "session.create",
            json!({ "workspace_id": moded["workspace_id"], "backend": "nope" }),
        )
        .await;
    assert_eq!(kind(&error), "backend_missing");
    let error = client
        .error("session.stop", json!({ "id": "s-missing" }))
        .await;
    assert_eq!(kind(&error), "not_found");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_backend_that_fails_to_start_ends_the_session_with_an_error() {
    let (core, _) = start_fake(FakeAdapter::failing()).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    // The exit came during the start; the failure that follows it ends
    // the session, and `session.ended` is the last event.
    let tail = &types(&events)[events.len() - 3..];
    assert_eq!(tail, ["error", "session.state", "session.ended"]);
    let ended = &events.last().unwrap()["event"];
    assert_eq!(ended["reason"], "error");
    assert_eq!(ended["exit_code"], 1);
    assert_eq!(session_row(&mut client, &id).await["state"], "error");
    let error = client
        .error("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
}

// verifies: FLOW-NEW-SESSION-R08
/// The first message of a session whose backend failed to start is answered
/// with the backend's own reason, whether it arrives while the start is
/// still under way or after the session has ended, so the new-session
/// screen can show why.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_to_a_backend_that_failed_to_start_is_answered_with_its_reason() {
    let (core, _) = start_fake(FakeAdapter::failing()).await;
    let mut client = core.client().await;

    let early = new_session(&core, &mut client).await;
    let error = client
        .error("session.send", json!({ "id": early, "text": "hello" }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("the backend could not be started")
            && message.contains("the fake refused to start"),
        "the backend's reason, got {message:?}"
    );

    // A second session in the same workspace, sent to once it has ended.
    let early_row = session_row(&mut client, &early).await;
    let late = client
        .result(
            "session.create",
            json!({ "workspace_id": early_row["workspace_id"], "backend": fake::BACKEND }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.subscribe", json!({ "id": late, "from_seq": 0 }))
        .await;
    client.events_until("session.ended").await;
    let error = client
        .error("session.send", json!({ "id": late, "text": "hello" }))
        .await;
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("the fake refused to start"),
        "the backend's reason after the end, got {message:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_adapter_that_panics_while_starting_fails_the_session_cleanly() {
    let (core, _) = start_fake(FakeAdapter::panicking()).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    assert_eq!(events.last().unwrap()["event"]["reason"], "error");
    // Not live any more: it can be removed.
    client.result("session.remove", json!({ "id": id })).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_waiting_on_one_session_does_not_hold_up_another() {
    let slow = FakeAdapter::slow_start(std::time::Duration::from_secs(3));
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(slow)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let params = json!({ "workspace_id": workspace_id, "backend": fake::BACKEND });
    let ready = client.result("session.create", params.clone()).await["id"].clone();
    // Wait for the first to start, and leave a permission waiting on it.
    client
        .result("session.send", json!({ "id": ready, "text": "ask" }))
        .await;
    let starting = client.result("session.create", params).await["id"].clone();

    // A message to the session still starting waits for it; a decision
    // for the other one, sent after it on the same connection, does not.
    let waiting = client
        .request("session.send", json!({ "id": starting, "text": "hello" }))
        .await;
    let decided = client
        .request(
            "session.decide_permission",
            json!({ "id": ready, "request_id": "r-1", "behavior": "allow" }),
        )
        .await;
    let decision = client.response(decided).await;
    assert!(decision.get("result").is_some(), "{decision:#}");
    assert!(
        client.responses_waiting() == 0,
        "the decision came back before the slow send"
    );
    let sent = client.response(waiting).await;
    assert_eq!(sent["result"]["turn"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn pipelined_requests_about_one_session_keep_their_order() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    // Sent back to back, none waiting for the one before it to be answered.
    let mut sends = Vec::new();
    for i in 0..300 {
        let text = format!("m{i}");
        sends.push(
            client
                .request("session.send", json!({ "id": id, "text": text }))
                .await,
        );
    }
    let interrupt = client
        .request("session.interrupt", json!({ "id": id }))
        .await;
    let stop = client.request("session.stop", json!({ "id": id })).await;
    let remove = client.request("session.remove", json!({ "id": id })).await;

    let mut turns = Vec::new();
    for request in sends {
        let response = client.response(request).await;
        turns.push(response["result"]["turn"].as_u64().expect("a turn"));
    }
    assert_eq!(turns, (1..=300).collect::<Vec<_>>());
    assert!(client.response(interrupt).await.get("result").is_some());
    assert_eq!(client.response(stop).await["result"]["state"], "ended");
    let removed = client.response(remove).await;
    assert_eq!(removed["result"]["id"], id.as_str(), "{removed:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_overtakes_requests_stuck_on_a_hung_start() {
    let hung = FakeAdapter::slow_start(std::time::Duration::from_secs(600));
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(hung)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    // Queued on the session: a message that waits for the start, then a
    // stop. Meanwhile the connection keeps reading: a request that names no
    // session is answered at once.
    let send = client
        .request("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    let listed = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.call("session.list", json!({})),
    )
    .await
    .expect("the connection is not held behind a session's lane");
    assert!(listed.get("result").is_some());
    let stop = client.request("session.stop", json!({ "id": id })).await;

    let started = std::time::Instant::now();
    let failed = client.response(send).await;
    assert_eq!(kind(&failed["error"]), "invalid_state", "{failed:#}");
    let stopped = client.response(stop).await;
    assert_eq!(stopped["result"]["state"], "ended", "{stopped:#}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(20),
        "the stop did not wait for the start"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resubscribe_delivers_exactly_from_its_seq_after_its_answer() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut driver = core.client().await;
    let id = new_session(&core, &mut driver).await;
    let mut client = core.client().await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    driver
        .result("session.send", json!({ "id": id, "text": "burst 3000" }))
        .await;
    // Resubscribe while the first subscription is mid-stream. What arrives
    // before the answer belongs to the old subscription; from the answer
    // on, the stream is the new one's alone.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 5 }))
        .await;
    client.clear_notifications();
    let mut seqs_seen = Vec::new();
    while seqs_seen.len() < 3000 {
        let n = client.notification().await.expect("open");
        if n["method"] == "session.event" {
            seqs_seen.push(n["params"]["event"]["seq"].as_u64().unwrap());
        }
    }
    assert_eq!(seqs_seen, (5..3005).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tile_carries_the_state_and_follows_it() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let mut watcher = core.client().await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    assert_eq!(
        session_row(&mut client, &id).await["tile"]["state"],
        "running"
    );
    client.result("session.stop", json!({ "id": id })).await;
    let ended = watcher
        .notification_where(|n| {
            n["method"] == "session.tile" && n["params"]["tile"]["state"] == "ended"
        })
        .await;
    assert_eq!(ended["params"]["session_id"], id.as_str());
    assert_eq!(
        session_row(&mut client, &id).await["tile"]["state"],
        "ended"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn re_projecting_is_gone_and_a_title_is_not_ersas_to_give() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client.result("session.stop", json!({ "id": id })).await;
    // Nothing is stored to re-project: the method is gone from the API.
    let error = client.error("session.reproject", json!({ "id": id })).await;
    assert_eq!(error["code"], -32601, "{error:#}");
    // session.create takes no title; the backend's record gives one.
    let workspace_id = session_row(&mut client, &id).await["workspace_id"].clone();
    let error = client
        .error(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND, "title": "mine" }),
        )
        .await;
    assert_eq!(error["code"], -32602, "{error:#}");
}

// verifies: ARCH-CORE-R12
// verifies: PROTO-AGENT-EVENTS-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_session_reopened_shows_its_record_and_turns_taken_elsewhere() {
    let records = Records::default();
    let (core, _) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;

    // Opened from the start with nobody else reading: the record, then how
    // Core saw the backend end. What only the live stream carried (the turn,
    // the usage) is not kept.
    let first = all_events(&mut client, &id).await;
    let summary = |events: &[Value]| -> Vec<(String, String)> {
        events
            .iter()
            .map(|e| {
                (
                    e["source"]["input"].as_str().unwrap().to_string(),
                    e["event"]["type"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    let expected = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    };
    assert_eq!(
        summary(&first),
        expected(&[
            ("record", "user.message"),
            ("record", "assistant.text"),
            ("core", "session.state"),
            ("core", "session.ended"),
        ])
    );
    assert_eq!(first[1]["event"]["text"], "echo: hello");
    assert_eq!(first[1]["source"]["message_id"], "r-2");
    // Opened again, the same record gives the same events.
    assert_eq!(all_events(&mut client, &id).await, first);

    // A turn taken in the backend's own tools appends to its record...
    let backend_id = records.lock().unwrap().keys().next().unwrap().clone();
    records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .extend([
            fake::recorded_user("u-t1", "from the terminal"),
            fake::recorded_reply("r-t2", "answered there"),
        ]);
    // ...and shows when the session is opened again.
    let reopened = all_events(&mut client, &id).await;
    assert_eq!(reopened.len(), first.len() + 2);
    assert_eq!(reopened[2]["event"]["text"], "from the terminal");
    assert_eq!(reopened[3]["event"]["text"], "answered there");
    assert_eq!(reopened.last().unwrap()["event"]["type"], "session.ended");
    assert_eq!(
        seqs(&reopened),
        (0..reopened.len() as u64).collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_resubscribing_keeps_the_view_it_was_reading() {
    let records = Records::default();
    let (core, _) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    let mut reader = core.client().await;
    reader
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let seen = reader.events_until("session.ended").await;
    let last = seqs(&seen).last().copied().unwrap();

    // The record grows, but a client resuming from where it was gets the
    // view it was reading, not a different one spliced in.
    let backend_id = records.lock().unwrap().keys().next().unwrap().clone();
    records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(fake::recorded_user("u-t1", "elsewhere"));
    let resumed = client
        .result(
            "session.subscribe",
            json!({ "id": id, "from_seq": last + 1 }),
        )
        .await;
    assert_eq!(resumed["last_seq"], last);
    // And a client opening it afresh while another reads it gets the same.
    let mut other = core.client().await;
    let fresh = other
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    assert_eq!(fresh["last_seq"], last);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_session_without_its_adapter_cannot_be_read_until_one_is_back() {
    let records = Records::default();
    let (core, _) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    drop(client);
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;

    // A Core without the fake adapter still ends it, in its app log.
    let core = start_with_adapters(dir, |_| {}, Vec::new()).await;
    let mut client = core.client().await;
    assert_eq!(session_row(&mut client, &id).await["state"], "ended");
    let error = client
        .error("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();

    // One with the adapter shows its history, ended by that restart.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(records))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let events = all_events(&mut client, &id).await;
    assert_eq!(types(&events)[..2], ["user.message", "assistant.text"]);
    assert_eq!(events.last().unwrap()["event"]["reason"], "error");
}

/// Every event of a session, by subscribing from 0 until `session.ended`.
async fn all_events(client: &mut Client, id: &str) -> Vec<Value> {
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let events = client.events_until("session.ended").await;
    client
        .result("session.unsubscribe", json!({ "id": id }))
        .await;
    events
}

// verifies: PROTO-AGENT-EVENTS-R13
// verifies: ARCH-CORE-R12
#[tokio::test(flavor = "multi_thread")]
async fn a_session_left_running_by_a_core_that_died_is_ended_on_restart() {
    let records = Records::default();
    let (core, _) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    drop(client);
    // No shutdown: the server stops without stopping the session.
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;

    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(records))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    assert_eq!(session_row(&mut client, &id).await["state"], "ended");
    // The transcript comes back from the record alone; the ending is Core's.
    let events = all_events(&mut client, &id).await;
    assert_eq!(seqs(&events), (0..events.len() as u64).collect::<Vec<_>>());
    let inputs: Vec<&str> = events
        .iter()
        .map(|e| e["source"]["input"].as_str().unwrap())
        .collect();
    assert_eq!(inputs, ["record", "record", "core", "core"]);
    assert_eq!(events[1]["event"]["text"], "echo: hello");
    let ended = &events.last().unwrap()["event"];
    assert_eq!(ended["reason"], "error");
    // This Core never ran the backend, so it has no raw log for it.
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    assert_eq!(log, json!([]));
    // And the app log says what happened.
    let facts: Vec<String> = ersa_core::app_log::read_entries(&core.data_dir())
        .unwrap()
        .into_iter()
        .filter(|e| e.session_id == id)
        .map(|e| {
            serde_json::to_value(&e.fact).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(facts, ["process_started", "core_restarted"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sessions_title_and_start_come_from_its_record_and_its_mode_from_ersa() {
    let records = Records::default();
    let (core, _) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.set_mode", json!({ "id": id, "mode": "review" }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["title"], "hello");
    assert_eq!(row["created_at"], "2026-11-01T00:00:00.000Z");
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();

    // A later Core reads them from the record again; the mode is Ersa's own.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(records))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["title"], "hello");
    assert_eq!(row["created_at"], "2026-11-01T00:00:00.000Z");
    assert_eq!(row["mode"], "review");
}

// verifies: PROTO-CORE-API-R12
#[tokio::test(flavor = "multi_thread")]
async fn a_running_session_is_renamed_through_its_backend() {
    let records = Records::default();
    let (core, stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(renamed["title"], "Renamed");
    assert_eq!(session_row(&mut client, &id).await["title"], "Renamed");
    // The running backend took it; nothing was written beside it.
    assert_eq!(stats.live_renames.load(Ordering::SeqCst), 1);
    assert_eq!(stats.record_renames.load(Ordering::SeqCst), 0);
    let raw = client.result("session.raw_log", json!({ "id": id })).await;
    assert!(
        raw.as_array()
            .unwrap()
            .iter()
            .any(|l| l["dir"] == "host" && l["line"].as_str().unwrap().contains("rename")),
        "{raw:#}"
    );
    // And a later message does not undo it: the backend keeps the title.
    client
        .result("session.send", json!({ "id": id, "text": "again" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    assert_eq!(session_row(&mut client, &id).await["title"], "Renamed");
}

// verifies: PROTO-CORE-API-R12
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_session_is_renamed_in_its_record_which_a_later_core_reads() {
    let records = Records::default();
    let (core, stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    assert_eq!(session_row(&mut client, &id).await["title"], "hello");

    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(renamed["title"], "Renamed");
    assert_eq!(session_row(&mut client, &id).await["title"], "Renamed");
    assert_eq!(stats.record_renames.load(Ordering::SeqCst), 1);
    assert_eq!(stats.live_renames.load(Ordering::SeqCst), 0);
    // The title is in the record, not in the conversation.
    let events = all_events(&mut client, &id).await;
    assert!(
        !events.iter().any(|e| e.to_string().contains("Renamed")),
        "{events:#?}"
    );
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();

    // Ersa keeps no title of its own: a later Core finds it in the record.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(records))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    assert_eq!(session_row(&mut client, &id).await["title"], "Renamed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_its_backend_never_named_cannot_be_renamed() {
    let (core, _) = start_fake(FakeAdapter::failing()).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.ended").await;
    let error = client
        .error("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    let error = client
        .error(
            "session.set_title",
            json!({ "id": "s-missing", "title": "x" }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blank_long_or_multi_line_title_is_refused() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    let long = "x".repeat(201);
    for title in ["", " \n ", "bell\u{7}", "nul\u{0}", long.as_str()] {
        let error = client
            .error("session.set_title", json!({ "id": id, "title": title }))
            .await;
        assert_eq!(error["code"], -32602, "{title:?}: {error:#}");
    }
    assert_eq!(stats.live_renames.load(Ordering::SeqCst), 0);
    // As long as the backend's own list shows, and trimmed.
    let longest = "é".repeat(200);
    let renamed = client
        .result(
            "session.set_title",
            json!({ "id": id, "title": format!(" {longest} ") }),
        )
        .await;
    assert_eq!(renamed["title"], longest.as_str());
    // Spaced as the backend's own list will show it.
    let renamed = client
        .result(
            "session.set_title",
            json!({ "id": id, "title": "Fix  the \t bug\non Windows\n" }),
        )
        .await;
    assert_eq!(renamed["title"], "Fix the bug on Windows");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_session_still_lists_as_failed_after_core_restarts() {
    let (core, _) = start_fake(FakeAdapter::failing()).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.ended").await;
    assert_eq!(session_row(&mut client, &id).await["state"], "error");
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();

    // Listed, not opened: its state comes from how Core saw it end.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["state"], "error");
    assert_eq!(row["tile"]["state"], "error");
}

// verifies: PROTO-CORE-API-R13
#[tokio::test(flavor = "multi_thread")]
async fn a_session_nobody_reads_lets_its_view_go_and_reopens_under_a_new_identity() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let live = client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    // While it runs, resubscribing keeps the view.
    let again = client
        .result("session.subscribe", json!({ "id": id, "from_seq": 3 }))
        .await;
    assert_eq!(again["view"], live["view"]);
    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    let held = client.result("session.raw_log", json!({ "id": id })).await;
    assert!(!held.as_array().unwrap().is_empty());

    // The last reader leaves: the view and raw log go.
    client
        .result("session.unsubscribe", json!({ "id": id }))
        .await;
    assert_eq!(
        client.result("session.raw_log", json!({ "id": id })).await,
        json!([])
    );
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["state"], "ended");
    assert!(row.get("last_event_seq").is_none(), "{row:#}");

    // A client coming back from where it was finds another view, and its
    // events start over from the record.
    let back = client
        .result("session.subscribe", json!({ "id": id, "from_seq": 9 }))
        .await;
    assert_ne!(back["view"], live["view"]);
    let fresh = all_events(&mut client, &id).await;
    assert_eq!(fresh[0]["seq"], 0);
    assert_eq!(fresh[0]["source"]["input"], "record");
}

/// The model, effort and permission mode a session names reach its
/// backend, and `session.list` reports them, also after Core restarts.
// verifies: PROTO-CORE-API-R16
#[tokio::test(flavor = "multi_thread")]
async fn a_session_starts_with_the_model_effort_and_permission_mode_it_names() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let created = client
        .result(
            "session.create",
            json!({
                "workspace_id": workspace_id,
                "backend": fake::BACKEND,
                "model": "model-b",
                "effort": "low",
                "permission_mode": "auto",
            }),
        )
        .await;
    let id = created["id"].as_str().unwrap().to_string();
    let chosen = |row: &Value| {
        (
            row["model"].clone(),
            row["effort"].clone(),
            row["permission_mode"].clone(),
        )
    };
    let named = (json!("model-b"), json!("low"), json!("auto"));
    assert_eq!(chosen(&created), named);
    assert_eq!(chosen(&session_row(&mut client, &id).await), named);
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    let config = stats.configs.lock().unwrap()[0].clone();
    assert_eq!(
        (
            config.model.as_deref(),
            config.effort.as_deref(),
            config.permission_mode.as_str()
        ),
        (Some("model-b"), Some("low"), "auto")
    );

    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    assert_eq!(chosen(&session_row(&mut client, &id).await), named);
}

/// A model or effort left out is passed as nothing and reported as absent,
/// also after Core restarts: the backend chooses it, and Core does not know
/// what it chose. A permission mode is always stated, so one left out is the
/// backend's default, and reported as the mode the session started with.
// verifies: PROTO-CORE-API-R16
#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_names_none_leaves_them_to_its_backend() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let absent = |row: &Value| {
        row.get("model").is_none()
            && row.get("effort").is_none()
            && row["permission_mode"] == json!("ask")
    };
    let row = session_row(&mut client, &id).await;
    assert!(absent(&row), "{row:#}");
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    let config = stats.configs.lock().unwrap()[0].clone();
    assert_eq!(
        (config.model, config.effort, config.permission_mode.as_str()),
        (None, None, "ask")
    );

    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let row = session_row(&mut client, &id).await;
    assert!(absent(&row), "{row:#}");
}

/// Only what `backend.list` offers reaches the backend.
#[tokio::test(flavor = "multi_thread")]
async fn a_choice_the_backend_does_not_offer_is_refused() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    for (field, value) in [
        ("model", "--dangerous"),
        ("effort", "extreme"),
        ("permission_mode", "bypassPermissions"),
    ] {
        let mut params = json!({ "workspace_id": workspace_id, "backend": fake::BACKEND });
        params[field] = json!(value);
        let error = client.error("session.create", params).await;
        assert_eq!(error["code"], json!(-32602), "{field}: {error}");
    }
    assert_eq!(client.result("session.list", json!({})).await, json!([]));
    assert_eq!(stats.starts.load(Ordering::SeqCst), 0);
}

/// A session can run in the project's own folder, and while one is there
/// the project cannot be removed.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_in_the_root_workspace_keeps_its_project() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let folder = core.dir.path().join("plain");
    std::fs::create_dir_all(&folder).unwrap();
    let project = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": folder.display().to_string(), "trusted": false }),
        )
        .await;
    let root = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await[0]
        .clone();
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": root["id"], "backend": fake::BACKEND }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    assert_eq!(
        stats.configs.lock().unwrap()[0].cwd,
        project["path"].as_str().unwrap()
    );

    let error = client
        .error("project.remove", json!({ "id": project["id"] }))
        .await;
    assert_eq!(kind(&error), "invalid_state");

    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    client.result("session.remove", json!({ "id": id })).await;
    client
        .result("project.remove", json!({ "id": project["id"] }))
        .await;
    assert!(
        folder.is_dir(),
        "the project's own folder is left as it was"
    );
}
