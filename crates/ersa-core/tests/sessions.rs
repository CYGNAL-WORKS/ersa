//! The session manager driven through the Core API over the real socket,
//! with a fake adapter (`support/fake.rs`) standing in for a backend:
//! sequencing, subscriptions, tiles, the raw log, stopping, and what a
//! restarted Core does with the sessions it finds.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Stats};
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
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND, "title": "test" }),
        )
        .await;
    assert_eq!(session["state"], "starting");
    assert_eq!(session["title"], "test");
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
        assert_eq!(event["v"], 1);
        assert_eq!(event["session_id"], id.as_str());
        assert_eq!(event["source"]["backend"], fake::BACKEND);
        let (from, to) = (&event["source"]["raw_from"], &event["source"]["raw_to"]);
        assert!(from.as_u64() <= to.as_u64());
    }
    assert_eq!(
        types(&events),
        [
            "session.state",
            "backend.passthrough",
            "session.state",
            "turn.started",
            "user.message",
            "usage.updated",
            "assistant.text"
        ]
    );
    assert_eq!(events[0]["event"]["state"], "starting");
    assert_eq!(events[2]["event"]["state"], "running");
    // `ts` is when the raw log recorded the event's last line.
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    for event in &events {
        let raw_to = event["source"]["raw_to"].as_u64().unwrap() as usize;
        assert_eq!(event["ts"], log[raw_to]["ts"], "{event:#}");
    }

    let second = client
        .result("session.send", json!({ "id": id, "text": "again" }))
        .await;
    assert_eq!(second["turn"], 2);
}

// verifies: PROTO-CORE-API-R02
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
#[tokio::test(flavor = "multi_thread")]
async fn the_raw_log_holds_every_line_exactly_as_it_crossed() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
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
    let reply = json!({ "t": "reply", "text": "echo: hello \"there\"" }).to_string();
    let adapter_lines: Vec<(&str, &str)> = lines
        .iter()
        .copied()
        .filter(|(dir, _)| *dir != "core")
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
    // Core's own records frame the session.
    assert_eq!(lines[0].0, "core");
    assert!(lines[0].1.contains("process_started"), "{}", lines[0].1);
    let last = lines.last().unwrap();
    assert_eq!(last.0, "core");
    assert!(last.1.contains("process_exited"), "{}", last.1);
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
async fn a_running_session_cannot_be_removed_and_titles_and_modes_change() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let error = client.error("session.remove", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state");
    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": "renamed" }))
        .await;
    assert_eq!(renamed["title"], "renamed");
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
async fn re_projecting_disturbs_subscribers_only_when_events_change() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut caller = core.client().await;
    let id = new_session(&core, &mut caller).await;
    caller
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    caller.result("session.stop", json!({ "id": id })).await;
    let mut watcher = core.client().await;
    for client in [&mut caller, &mut watcher] {
        client
            .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
            .await;
        client.events_until("session.ended").await;
    }

    // The same version re-derives the same events: nobody is disturbed,
    // and the caller, itself subscribed, gets its answer.
    let row = caller
        .result("session.reproject", json!({ "id": id }))
        .await;
    assert_eq!(row["id"], id.as_str());
    caller.result("session.list", json!({})).await;
    watcher.result("session.list", json!({})).await;

    // A cached event that no longer matches the raw log is a change: other
    // subscribers are disconnected, the caller is answered and kept.
    let conn = rusqlite::Connection::open(core.data_dir().join("ersa.db")).unwrap();
    conn.execute(
        "UPDATE projection_cache SET envelope = replace(envelope, 'echo: hello', 'tampered')
         WHERE session_id = ?1",
        [&id],
    )
    .unwrap();
    let row = caller
        .result("session.reproject", json!({ "id": id }))
        .await;
    assert_eq!(row["state"], "ended");
    while watcher.read_raw().await.is_some() {}
    caller.result("session.list", json!({})).await;
    // And the caller reads the re-derived events by subscribing again.
    let events = all_events(&mut caller, &id).await;
    assert!(events.iter().any(|e| e["event"]["text"] == "echo: hello"));
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
    // stop. A subscribe in between is answered at once, in line.
    let send = client
        .request("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    let subscribed = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.call("session.subscribe", json!({ "id": id, "from_seq": 0 })),
    )
    .await
    .expect("an in-line request is not held behind a detached one");
    assert!(subscribed.get("result").is_some());
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

/// Sets a session's row state directly, as a Core that wrote the row but
/// not the session's ending would leave it.
fn set_row_state(core_dir: &std::path::Path, id: &str, state: &str) {
    let conn = rusqlite::Connection::open(core_dir.join("data").join("ersa.db")).unwrap();
    conn.execute(
        "UPDATE sessions SET state = ?2 WHERE id = ?1",
        rusqlite::params![id, state],
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_whose_ending_was_never_recorded_is_ended_whatever_its_row_says() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    drop(client);
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;
    // The row says `error`, as when the raw log could not be written, but
    // nothing in the log ends the session.
    set_row_state(dir.path(), &id, "error");

    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let events = all_events(&mut client, &id).await;
    assert_eq!(events.last().unwrap()["event"]["reason"], "error");
    assert_eq!(session_row(&mut client, &id).await["state"], "ended");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_session_without_its_adapter_is_ended_and_projected_later() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    drop(client);
    let TestCore { dir, task, .. } = core;
    task.abort();
    let _ = task.await;

    // A Core without the fake adapter ends it in the log and on its row.
    let core = start_with_adapters(dir, |_| {}, Vec::new()).await;
    let mut client = core.client().await;
    assert_eq!(session_row(&mut client, &id).await["state"], "ended");
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let last = log.as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["line"], r#"{"type":"core_restarted"}"#);
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();

    // One with the adapter projects that ending.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let events = all_events(&mut client, &id).await;
    assert_eq!(seqs(&events), (0..events.len() as u64).collect::<Vec<_>>());
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

/// The projector versions of a session's cached events, read straight from
/// the database.
fn cached_versions(core_dir: &std::path::Path, id: &str) -> Vec<i64> {
    let conn = rusqlite::Connection::open(core_dir.join("data").join("ersa.db")).unwrap();
    let mut stmt = conn
        .prepare("SELECT DISTINCT projection_version FROM projection_cache WHERE session_id = ?1")
        .unwrap();
    stmt.query_map([id], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

// verifies: PROTO-AGENT-EVENTS-R03
#[tokio::test(flavor = "multi_thread")]
async fn a_new_projector_version_rederives_the_same_events_from_the_raw_log() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "ask" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    let before = all_events(&mut client, &id).await;
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    assert_eq!(cached_versions(dir.path(), &id), [1]);

    // The same data under a Core whose projector has a new version.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(2))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    assert_eq!(cached_versions(core.dir.path(), &id), [2]);
    let mut client = core.client().await;
    let after = all_events(&mut client, &id).await;
    assert_eq!(after, before);

    // And on request.
    client
        .result("session.reproject", json!({ "id": id }))
        .await;
    assert_eq!(all_events(&mut client, &id).await, before);
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["last_event_seq"], *seqs(&before).last().unwrap());
    assert_eq!(row["state"], "ended");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_left_running_by_a_core_that_died_is_ended_on_restart() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
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

    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    assert_eq!(session_row(&mut client, &id).await["state"], "ended");
    let events = all_events(&mut client, &id).await;
    assert_eq!(seqs(&events), (0..events.len() as u64).collect::<Vec<_>>());
    let ended = &events.last().unwrap()["event"];
    assert_eq!(ended["reason"], "error");
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let last = log.as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["dir"], "core");
    assert_eq!(last["line"], r#"{"type":"core_restarted"}"#);
}
