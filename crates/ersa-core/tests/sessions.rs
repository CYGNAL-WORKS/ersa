//! The session manager driven through the Core API over the real socket,
//! with a fake adapter (`support/fake.rs`) standing in for a backend:
//! sequencing, subscriptions, tiles, the raw log, stopping, what a restarted
//! Core does with the sessions it finds, and opening a session from its
//! backend's record.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_adapter::OpenElsewhere;
use ersa_core::Adapter;
use ersa_protocol::events::PROJECTION_VERSION;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Records, Stats};
use support::{
    Client, TestCore, app_log_facts, kind, restarted, start_with_adapters, tempdir, workspace,
};

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

/// A new session of the fake backend, subscribed, started and idle after a
/// first turn: one Archive takes (PROTO-CORE-API-R77). Returns its id.
async fn idle_session(core: &TestCore, client: &mut Client) -> String {
    let id = new_session(core, client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while session_row(client, &id).await["state"] != "idle" {
        assert!(tokio::time::Instant::now() < deadline, "{id} never idle");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    id
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

/// The session's row once Core has read its title from the record. Core
/// reads it after `session.stop` has its answer, so that a record that is
/// slow to read never holds up the stop, and announces nothing when the
/// title lands: the row is read again until it has one.
async fn titled_row(client: &mut Client, id: &str) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let row = session_row(client, id).await;
        if !row["title"].is_null() {
            return row;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "session {id} got no title from its record within 10 s"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
        assert_eq!(event["v"], json!(PROJECTION_VERSION));
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
    app_log_facts(
        &core.data_dir(),
        &id,
        &["process_started", "process_exited"],
    )
    .await;
    let indices: Vec<u64> = log.iter().map(|e| e["index"].as_u64().unwrap()).collect();
    assert_eq!(indices, (0..log.len() as u64).collect::<Vec<_>>());

    let window = client
        .result("session.raw_log", json!({ "id": id, "from": 2, "to": 3 }))
        .await;
    assert_eq!(window.as_array().unwrap(), &log[2..=3]);
}

/// The indices of raw log entries.
fn raw_indices(log: &[Value]) -> Vec<u64> {
    log.iter().map(|e| e["index"].as_u64().unwrap()).collect()
}

/// The session's raw log, read with `params` added to its id.
async fn raw_log(client: &mut Client, id: &str, params: Value) -> Vec<Value> {
    let mut params = params;
    params["id"] = json!(id);
    let log = client.result("session.raw_log", params).await;
    log.as_array().unwrap().clone()
}

/// A new session with a reader, so Core holds its raw log after it stops.
async fn read_session(core: &TestCore, client: &mut Client) -> String {
    let id = new_session(core, client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    id
}

// verifies: PROTO-CORE-API-R136
// verifies: PROTO-CORE-API-R137
#[tokio::test(flavor = "multi_thread")]
async fn a_backend_started_again_keeps_its_earlier_lines_and_counts_on() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = read_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "first" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    let first = raw_log(&mut client, &id, json!({})).await;
    assert!(!first.is_empty());

    // A send to the stopped session starts its backend again.
    client
        .result("session.send", json!({ "id": id, "text": "second" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    let both = raw_log(&mut client, &id, json!({})).await;

    // The first run's lines are still there, as they were.
    assert_eq!(&both[..first.len()], &first[..]);
    // The second run's lines follow, numbered on from the first's.
    assert!(both.len() > first.len());
    assert_eq!(
        raw_indices(&both),
        (0..both.len() as u64).collect::<Vec<_>>()
    );
    let second = json!({ "t": "user", "text": "second" }).to_string();
    assert!(both[first.len()..].iter().any(|e| e["line"] == second));
}

// verifies: PROTO-CORE-API-R136
#[tokio::test(flavor = "multi_thread")]
async fn a_raw_log_held_again_never_reuses_an_index() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = read_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "first" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    let before = raw_indices(&raw_log(&mut client, &id, json!({})).await);
    let last = *before.last().unwrap();

    // Nobody reads it: Core lets the raw log go.
    client
        .result("session.unsubscribe", json!({ "id": id }))
        .await;
    assert!(raw_log(&mut client, &id, json!({})).await.is_empty());

    // Held again, its lines count on from the last index it had.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "second" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    let after = raw_indices(&raw_log(&mut client, &id, json!({})).await);
    assert!(!after.is_empty());
    assert_eq!(
        after,
        (last + 1..last + 1 + after.len() as u64).collect::<Vec<_>>(),
        "{before:?} then {after:?}"
    );
}

// verifies: PROTO-CORE-API-R138
#[tokio::test(flavor = "multi_thread")]
async fn a_clipped_read_cuts_long_lines_on_a_whole_character() {
    /// The longest start of `line` of at most `clip` bytes on a character
    /// boundary, worked out here rather than by Core's helper.
    fn expected_head(line: &str, clip: usize) -> &str {
        let mut end = clip;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        &line[..end]
    }

    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = read_session(&core, &mut client).await;
    // Two-byte characters after a 21-byte start (`{"t":"user","text":"a`):
    // byte 1024 falls inside one.
    let text = format!("a{}", "é".repeat(1000));
    client
        .result("session.send", json!({ "id": id, "text": text }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    let whole = raw_log(&mut client, &id, json!({})).await;
    let host = json!({ "t": "user", "text": text }).to_string();
    assert!(host.starts_with(r#"{"t":"user","text":""#) && host.len() > 1024);
    let host_at = whole.iter().position(|e| e["line"] == host).unwrap();

    for clip in [1024usize, host.len()] {
        let clipped = raw_log(&mut client, &id, json!({ "clip": clip })).await;
        assert_eq!(clipped.len(), whole.len());
        let mut cut = 0;
        for (got, full) in clipped.iter().zip(&whole) {
            assert_eq!(got["index"], full["index"]);
            assert_eq!(got["ts"], full["ts"]);
            assert_eq!(got["dir"], full["dir"]);
            let line = full["line"].as_str().unwrap();
            if line.len() > clip {
                cut += 1;
                assert!(got.get("line").is_none(), "{got:#}");
                assert_eq!(got["head"], expected_head(line, clip));
                assert_eq!(got["size"], line.len());
            } else {
                // A line of `clip` bytes or fewer comes back whole.
                assert_eq!(got["line"], line);
                assert!(got.get("head").is_none() && got.get("size").is_none());
            }
        }
        if clip == 1024 {
            // The user's line and the echoed reply, both cut.
            assert_eq!(cut, 2);
            assert_eq!(clipped[host_at]["head"].as_str().unwrap().len(), 1023);
        } else {
            // Exactly `clip` bytes: the user's line comes back whole.
            assert_eq!(clipped[host_at]["line"], host);
            assert_eq!(cut, 1);
        }
    }

    // The bounds are inclusive; anything outside them is refused.
    for clip in [1024, 1024 * 1024] {
        raw_log(&mut client, &id, json!({ "clip": clip })).await;
    }
    for clip in [json!(0), json!(1023), json!(1024 * 1024 + 1), json!(-1)] {
        let error = client
            .error("session.raw_log", json!({ "id": id, "clip": clip }))
            .await;
        assert_eq!(error["code"], -32602, "clip {clip}: {error:#}");
    }
}

// verifies: PROTO-CORE-API-R139
#[tokio::test(flavor = "multi_thread")]
async fn a_raw_log_answer_is_bounded_by_its_size_as_sent() {
    const CAP: usize = 8 * 1024 * 1024;
    /// What entries take in an answer: their JSON and a comma each.
    fn sent(entries: &[Value]) -> usize {
        entries
            .iter()
            .map(|e| serde_json::to_string(e).unwrap().len() + 1)
            .sum()
    }
    fn text(entries: &[Value]) -> usize {
        entries
            .iter()
            .map(|e| e["line"].as_str().unwrap().len())
            .sum()
    }

    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = read_session(&core, &mut client).await;
    // A control character is six bytes in a line ("\u0001") and seven in
    // the entry that carries it: four such lines are under 8 MiB as text
    // and over it as sent.
    let long = "\u{1}".repeat(330_000);
    for _ in 0..2 {
        client
            .result("session.send", json!({ "id": id, "text": long }))
            .await;
    }
    client.result("session.stop", json!({ "id": id })).await;

    let first = raw_log(&mut client, &id, json!({})).await;
    assert!(sent(&first) <= CAP, "{} bytes as sent", sent(&first));

    // Paging on from the last index gets the rest, the second echo last.
    let next = raw_indices(&first).last().unwrap() + 1;
    let rest = raw_log(&mut client, &id, json!({ "from": next })).await;
    assert!(!rest.is_empty(), "the first answer held every line");
    let all: Vec<Value> = first.iter().chain(&rest).cloned().collect();
    assert_eq!(raw_indices(&all), (0..all.len() as u64).collect::<Vec<_>>());
    let echoes = |entries: &[Value]| {
        entries
            .iter()
            .filter(|e| {
                let line = e["line"].as_str().unwrap();
                line.contains(r#""t":"reply""#) && line.len() > long.len()
            })
            .count()
    };
    assert_eq!(echoes(&all), 2);
    assert!(echoes(&rest) >= 1);
    // Under 8 MiB as text: only what it takes as sent split the answer.
    assert!(text(&all) < CAP, "{} bytes of text", text(&all));
    assert!(sent(&all) > CAP);
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

    // Starting it again is `session.resume`, which `session.start` names.
    let error = client.error("session.start", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state");
    assert_eq!(error["data"]["reason"], "session_not_running");
    assert_eq!(stats.starts.load(Ordering::SeqCst), 1, "not started again");

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
// verifies: PROTO-CORE-API-R88
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
    // No `reason`: the Interface shows the backend's own words, such as
    // not being signed in, rather than its words for a stopped session.
    assert!(error["data"].get("reason").is_none(), "{error:#}");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("the backend could not be started")
            && message.contains("the fake refused to start"),
        "the backend's reason, got {message:?}"
    );
    // Answered once the run has ended, so the listing already says so: a
    // failed start reads apart from a slow one by its state.
    assert_eq!(session_row(&mut client, &early).await["state"], "error");

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
    assert!(error["data"].get("reason").is_none(), "{error:#}");
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
    // It has ended: it can be removed.
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

/// A stop keeps its place behind messages sent before it for as long as
/// they keep going through, however long that takes all told: each is
/// answered with its turn, none starts the session again, and the stop then
/// ends it. Together the messages take longer than the 5 seconds a stop
/// waits behind requests that do not move. A stop waits for the last
/// message's slot, not its answer (PROTO-CORE-API, Sending), so the last
/// one may be cut short, but it never starts the session again either.
// verifies: PROTO-CORE-API-R117
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_waits_behind_messages_that_keep_going_through() {
    const SLOW: u32 = 20;
    assert!(fake::SLOW_HAND_OVER * SLOW > Duration::from_secs(7));
    let adapter = FakeAdapter::new(1);
    let stats = Arc::clone(&adapter.stats);
    let (core, _) = start_fake(adapter).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let mut sends = Vec::new();
    for _ in 0..SLOW {
        sends.push(
            client
                .request("session.send", json!({ "id": id, "text": "slow" }))
                .await,
        );
    }
    let last = client
        .request("session.send", json!({ "id": id, "text": "last" }))
        .await;
    let stop = client.request("session.stop", json!({ "id": id })).await;

    let mut turns = Vec::new();
    for request in sends {
        let response = client.response(request).await;
        turns.push(response["result"]["turn"].clone());
    }
    assert_eq!(
        turns,
        (1..=SLOW).map(|turn| json!(turn)).collect::<Vec<_>>()
    );
    let last = client.response(last).await;
    assert!(
        [json!(SLOW + 1), Value::Null].contains(&last["result"]["turn"]),
        "{last:#}"
    );
    assert_eq!(client.response(stop).await["result"]["state"], "ended");
    assert_eq!(
        stats.starts.load(Ordering::SeqCst),
        1,
        "nothing started it again"
    );
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
    app_log_facts(
        &core.data_dir(),
        &id,
        &["process_started", "core_restarted"],
    )
    .await;
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
    let row = titled_row(&mut client, &id).await;
    assert_eq!(row["title"], "hello");
    assert_eq!(row["created_at"], "2026-11-01T00:00:00.000Z");

    // A later Core reads them from the record again; the mode is Ersa's own.
    let (_core, mut client) = restarted(core, FakeAdapter::with_records(records)).await;
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
    assert_eq!(titled_row(&mut client, &id).await["title"], "hello");

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

    // Ersa keeps no title of its own: a later Core finds it in the record.
    let (_core, mut client) = restarted(core, FakeAdapter::with_records(records)).await;
    assert_eq!(session_row(&mut client, &id).await["title"], "Renamed");
}

/// A rename from one window reaches every other at once, as a tile carrying
/// the new title, for a stopped session too.
// verifies: PROTO-CORE-API-R83
#[tokio::test(flavor = "multi_thread")]
async fn a_rename_sends_every_client_a_tile_carrying_the_title() {
    let records = Records::default();
    let (core, _stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    titled_row(&mut client, &id).await;

    let mut other = core.client().await;
    client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    let tile = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        other.notification_where(|n| {
            n["method"] == "session.tile" && n["params"]["session_id"] == id
        }),
    )
    .await
    .expect("a tile within 10 s");
    assert_eq!(tile["params"]["title"], "Renamed", "{tile:#}");
}

/// The record read at a session's end gives it a title its tiles never
/// carried (the fake backend announces none), and every window is sent it.
// verifies: PROTO-CORE-API-R167
#[tokio::test(flavor = "multi_thread")]
async fn a_title_read_from_the_record_at_the_end_is_sent_as_a_tile() {
    let records = Records::default();
    let (core, _stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let mut other = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    assert_eq!(session_row(&mut client, &id).await["title"], Value::Null);
    client.result("session.stop", json!({ "id": id })).await;
    let tile = tokio::time::timeout(
        Duration::from_secs(10),
        other.notification_where(|n| {
            n["method"] == "session.tile"
                && n["params"]["session_id"] == id
                && n["params"]["title"] == "hello"
        }),
    )
    .await
    .expect("a tile carrying the record's title within 10 s");
    assert_eq!(tile["params"]["title"], "hello");
}

/// The record read at a session's end corrects the date created Core knew
/// (when it started the backend) to the record's first entry, and every
/// window is sent a tile carrying the corrected date.
// verifies: PROTO-CORE-API-R106
#[tokio::test(flavor = "multi_thread")]
async fn a_date_created_corrected_by_the_record_is_sent_as_a_tile() {
    let records = Records::default();
    let (core, _stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let mut other = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    // Named first, so the record read at the end finds the title the last
    // tile carried, and only the date created can call for a tile.
    client
        .result("session.set_title", json!({ "id": id, "title": "Named" }))
        .await;
    let before = session_row(&mut client, &id).await["created_at"].clone();
    assert_ne!(before, "2026-11-01T00:00:00.000Z");
    client.result("session.stop", json!({ "id": id })).await;
    let tile = tokio::time::timeout(
        Duration::from_secs(10),
        other.notification_where(|n| {
            n["method"] == "session.tile"
                && n["params"]["session_id"] == id
                && n["params"]["created_at"] == "2026-11-01T00:00:00.000Z"
        }),
    )
    .await
    .expect("a tile carrying the record's date created within 10 s");
    assert_eq!(tile["params"]["title"], "Named", "{tile:#}");
}

/// Every tile carries the date created `session.list` lists at the time:
/// the one Core knew while the session runs, the record's once read, and
/// the same on a tile for a rename.
// verifies: PROTO-CORE-API-R105
#[tokio::test(flavor = "multi_thread")]
async fn every_tile_carries_the_date_created_the_list_gives() {
    let records = Records::default();
    let (core, _stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let mut other = core.client().await;
    let id = new_session(&core, &mut client).await;
    let tile_for = |n: &Value| n["method"] == "session.tile" && n["params"]["session_id"] == id;
    let first = tokio::time::timeout(Duration::from_secs(10), other.notification_where(&tile_for))
        .await
        .expect("a tile within 10 s");
    let listed = session_row(&mut client, &id).await["created_at"].clone();
    assert!(listed.is_string(), "{listed}");
    assert_eq!(first["params"]["created_at"], listed, "{first:#}");

    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    // Once the record has been read, a rename's tile carries its date.
    titled_row(&mut client, &id).await;
    client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    let renamed = tokio::time::timeout(
        Duration::from_secs(10),
        other.notification_where(|n| tile_for(n) && n["params"]["title"] == "Renamed"),
    )
    .await
    .expect("a tile within 10 s");
    let listed = session_row(&mut client, &id).await["created_at"].clone();
    assert_eq!(listed, "2026-11-01T00:00:00.000Z");
    assert_eq!(renamed["params"]["created_at"], listed, "{renamed:#}");
}

/// Opening a stopped session's view tells every window of its tile, and
/// that tile carries the title its record gives it.
// verifies: PROTO-CORE-API-R83
#[tokio::test(flavor = "multi_thread")]
async fn the_tile_sent_when_a_stopped_session_opens_carries_its_title() {
    let records = Records::default();
    let (core, _stats) = start_fake(FakeAdapter::with_records(Arc::clone(&records))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;

    // A later Core, whose view of the session opens on a subscription.
    let (core, mut client) = restarted(core, FakeAdapter::with_records(records)).await;
    let mut other = core.client().await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let tile = tokio::time::timeout(
        Duration::from_secs(10),
        other.notification_where(|n| {
            n["method"] == "session.tile" && n["params"]["session_id"] == id
        }),
    )
    .await
    .expect("a tile within 10 s");
    assert_eq!(tile["params"]["title"], "hello", "{tile:#}");
}

/// A stopped session, renamed while the fake's registry answers `answer`:
/// the events that reached a client reading the session as it was renamed,
/// after the session's end.
async fn rename_while_open_elsewhere(answer: OpenElsewhere) -> (Vec<Value>, Arc<Stats>, String) {
    let adapter = FakeAdapter::new(1);
    *adapter.open_elsewhere.lock().unwrap() = answer;
    let (core, stats) = start_fake(adapter).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.ended").await;

    client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(stats.record_renames.load(Ordering::SeqCst), 1);
    // Whatever the answer, Core asked once, before it wrote the record.
    let checks = stats.open_checks.lock().unwrap().clone();
    assert_eq!(checks.len(), 1, "{checks:?}");
    assert!(checks[0].0.starts_with("fake-"), "{checks:?}");
    assert_eq!(checks[0].1, 0, "asked after the record was written");

    // Anything Core published for the rename reaches the client in a
    // moment; wait a little past that for what does.
    let mut after = Vec::new();
    while let Ok(Some(n)) =
        tokio::time::timeout(std::time::Duration::from_millis(300), client.notification()).await
    {
        if n["method"] == "session.event" {
            after.push(n["params"]["event"].clone());
        }
    }
    (after, stats, id)
}

// verifies: PROTO-AGENT-EVENTS-R15
// verifies: PROTO-AGENT-EVENTS-R16
#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_session_another_process_has_open_says_so_and_renames_it() {
    let (after, _, _) = rename_while_open_elsewhere(OpenElsewhere::Yes { pids: vec![4242] }).await;
    assert_eq!(types(&after), ["session.notice"], "{after:#?}");
    let notice = &after[0];
    assert_eq!(notice["event"]["code"], "session_open_elsewhere");
    assert_eq!(notice["event"]["level"], "warning");
    assert_eq!(notice["event"]["pid"], 4242);
    assert!(
        notice["event"]["message"]
            .as_str()
            .unwrap()
            .contains("4242"),
        "{notice:#}"
    );
    assert_eq!(notice["source"]["input"], "core");
}

// verifies: PROTO-AGENT-EVENTS-R15
// verifies: PROTO-AGENT-EVENTS-R16
#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_session_when_the_check_cannot_tell_says_so_and_renames_it() {
    let log = support::log::Log::installed();
    let (after, _, id) = rename_while_open_elsewhere(OpenElsewhere::Unknown).await;
    assert_eq!(types(&after), ["session.notice"], "{after:#?}");
    let notice = &after[0]["event"];
    assert_eq!(notice["code"], "session_open_check_failed");
    assert_eq!(notice["level"], "info");
    assert!(notice.get("pid").is_none(), "{notice:#}");
    // The diagnostic log says why it could not tell, for this session.
    let why = log.warning("could not tell whether another process", &id);
    assert!(why.contains("the adapter could not tell"), "{why}");
}

// verifies: PROTO-AGENT-EVENTS-R15
#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_session_no_other_process_has_open_gives_no_notice() {
    let (after, _, _) = rename_while_open_elsewhere(OpenElsewhere::No).await;
    assert!(after.is_empty(), "{after:#?}");
}

// verifies: PROTO-AGENT-EVENTS-R15
#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_running_session_asks_no_registry() {
    // Ersa's own process has it open: the rename goes through it.
    let adapter = FakeAdapter::new(1);
    *adapter.open_elsewhere.lock().unwrap() = OpenElsewhere::Yes { pids: vec![4242] };
    let (core, stats) = start_fake(adapter).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert!(stats.open_checks.lock().unwrap().is_empty());
}

// verifies: PROTO-CORE-API-R79
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
    assert_eq!(error["data"]["reason"], "backend_unnamed", "{error:#}");
    let error = client
        .error(
            "session.set_title",
            json!({ "id": "s-missing", "title": "x" }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");
}

/// A session stops being live once its `session.ended` is sent, so a
/// rename a client asks for after seeing it is a stopped session's: written
/// to the record, or refused for what the record lacks, never refused as
/// not running while Core is still letting go of the run.
// verifies: PROTO-CORE-API-R130
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_rename_after_session_ended_is_a_stopped_sessions_while_the_run_winds_down() {
    // Core holds the run this long after announcing its end: the window a
    // busy machine opens by chance.
    let winding_down = |config: &mut ersa_core::CoreConfig| {
        config.wind_down_pause = Duration::from_secs(2);
    };

    // Named: its backend exited on its own, and the record takes the title.
    let records = Records::default();
    let adapter = FakeAdapter::with_records(Arc::clone(&records));
    let stats = Arc::clone(&adapter.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), winding_down, adapters).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "exit" }))
        .await;
    client.events_until("session.ended").await;
    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(renamed["title"], "Renamed");
    assert_eq!(stats.record_renames.load(Ordering::SeqCst), 1);
    assert_eq!(stats.live_renames.load(Ordering::SeqCst), 0);

    // Never named: its start failed, so there is nothing to rename.
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::failing())];
    let core = start_with_adapters(tempdir(), winding_down, adapters).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.ended").await;
    let error = client
        .error("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(error["data"]["reason"], "backend_unnamed", "{error:#}");
}

/// A read of the record that took the old title before a rename, and lands
/// after it, leaves the rename's title: the title is whichever came last,
/// and the rename did. As when a session that just ended is renamed while
/// Core is still reading its record.
// verifies: PROTO-CORE-API-R168
#[tokio::test(flavor = "multi_thread")]
async fn a_record_read_from_before_a_rename_leaves_the_new_title() {
    let records = Records::default();
    let adapter = FakeAdapter::with_records(Arc::clone(&records));
    let (gate, stats) = (
        Arc::clone(&adapter.summary_gate),
        Arc::clone(&adapter.stats),
    );
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "exit" }))
        .await;
    client.events_until("session.ended").await;
    // The record's title, once the read at the run's end has landed.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while support::listed(&mut client, &id)
        .await
        .map(|s| s["title"].clone())
        != Some(json!("hello"))
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the record's title is listed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // A list reads the record, slowly: it is held once read.
    let held = gate.lock().await;
    let before = stats.summaries.load(Ordering::SeqCst);
    let mut lister = core.client().await;
    let listing = lister.request("session.list", json!({})).await;
    while stats.summaries.load(Ordering::SeqCst) == before {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the list reads the record"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let renamed = client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(renamed["title"], "Renamed");

    // The read lands: the rename's title stands.
    drop(held);
    let listed = lister.response(listing).await;
    let row = listed["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id.as_str())
        .cloned()
        .unwrap();
    assert_eq!(row["title"], "Renamed", "{row:#}");
}

/// The same for a title the backend announced on its live stream, as it
/// does when it takes a rename before Core has set the rename's title: a
/// read of the record begun before the announcement, here while the
/// session was stopped, leaves it once the session runs again.
// verifies: PROTO-CORE-API-R168
#[tokio::test(flavor = "multi_thread")]
async fn a_record_read_from_before_an_announced_title_leaves_it() {
    let records = Records::default();
    let adapter = FakeAdapter::with_records(Arc::clone(&records));
    let (gate, stats) = (
        Arc::clone(&adapter.summary_gate),
        Arc::clone(&adapter.stats),
    );
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    // The record's title, once a list has read it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while support::listed(&mut client, &id)
        .await
        .map(|s| s["title"].clone())
        != Some(json!("hello"))
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the record's title is listed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // A list reads the record, slowly: it is held once read.
    let held = gate.lock().await;
    let before = stats.summaries.load(Ordering::SeqCst);
    let mut lister = core.client().await;
    let listing = lister.request("session.list", json!({})).await;
    while stats.summaries.load(Ordering::SeqCst) == before {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the list reads the record"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "announce Renamed" }),
        )
        .await;
    client
        .notification_where(|n| {
            n["method"] == "session.tile"
                && n["params"]["session_id"] == id.as_str()
                && n["params"]["title"] == "Renamed"
        })
        .await;

    // The read lands: the announced title stands.
    drop(held);
    let listed = lister.response(listing).await;
    let row = listed["result"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id.as_str())
        .cloned()
        .unwrap();
    assert_eq!(row["title"], "Renamed", "{row:#}");
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

/// A title is held to the rules its backend's adapter states: its limit, and
/// whether a line break is kept or taken as a space.
// verifies: PROTO-CORE-API-R70
#[tokio::test(flavor = "multi_thread")]
async fn a_title_follows_the_rules_its_backends_adapter_states() {
    use ersa_adapter::TitleRules;
    for one_line in [true, false] {
        let mut adapter = FakeAdapter::new(1);
        adapter.title_rules = TitleRules {
            max_chars: 25,
            one_line,
        };
        let (core, _) = start_fake(adapter).await;
        let mut client = core.client().await;
        let id = new_session(&core, &mut client).await;
        client
            .result("session.send", json!({ "id": id, "text": "hello" }))
            .await;
        let error = client
            .error(
                "session.set_title",
                json!({ "id": id, "title": "y".repeat(26) }),
            )
            .await;
        assert_eq!(error["code"], -32602, "{error:#}");
        assert!(
            error["message"].as_str().unwrap().contains("25"),
            "{error:#}"
        );
        let longest = "y".repeat(25);
        let renamed = client
            .result("session.set_title", json!({ "id": id, "title": longest }))
            .await;
        assert_eq!(renamed["title"], longest.as_str());

        let renamed = client
            .result(
                "session.set_title",
                json!({ "id": id, "title": "\n Fix  the \t bug \r\non Windows\n" }),
            )
            .await;
        let expected = if one_line {
            "Fix the bug on Windows"
        } else {
            "Fix the bug\non Windows"
        };
        assert_eq!(renamed["title"], expected, "one line: {one_line}");
        // Spacing is spacing either way; another control character is not.
        let error = client
            .error(
                "session.set_title",
                json!({ "id": id, "title": "bell\u{7}" }),
            )
            .await;
        assert_eq!(error["code"], -32602, "{error:#}");
    }
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
    drop(client);

    // Listed, not opened: its state comes from how Core saw it end.
    let (_core, mut client) = restarted(core, FakeAdapter::new(1)).await;
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

/// Whether `workspace.list` still lists the workspace `workspace_id`, in
/// any project.
/// The folder of the workspace with this id, as `workspace.list` gives it.
async fn workspace_path(client: &mut Client, workspace_id: &Value) -> std::path::PathBuf {
    let projects = client.result("project.list", json!({})).await;
    for project in projects.as_array().unwrap() {
        let workspaces = client
            .result("workspace.list", json!({ "project_id": project["id"] }))
            .await;
        if let Some(workspace) = workspaces
            .as_array()
            .unwrap()
            .iter()
            .find(|w| &w["id"] == workspace_id)
        {
            return std::path::PathBuf::from(workspace["path"].as_str().unwrap());
        }
    }
    panic!("no workspace {workspace_id}");
}

async fn workspace_listed(client: &mut Client, workspace_id: &Value) -> bool {
    let projects = client.result("project.list", json!({})).await;
    for project in projects.as_array().unwrap() {
        let workspaces = client
            .result("workspace.list", json!({ "project_id": project["id"] }))
            .await;
        if workspaces
            .as_array()
            .unwrap()
            .iter()
            .any(|w| &w["id"] == workspace_id)
        {
            return true;
        }
    }
    false
}

/// Archiving a running session stops it, as `session.stop` does, and marks
/// it archived; the session keeps its pin and its section, and the archive
/// removes nothing: the workspace and its folder stay.
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R12
#[tokio::test(flavor = "multi_thread")]
async fn archiving_a_running_session_stops_it_and_keeps_it_and_its_workspace() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    // Ready, so the stop reaches the backend rather than cancelling its
    // start, and idle, since a busy session can't be archived.
    let id = idle_session(&core, &mut client).await;
    let workspace_id = session_row(&mut client, &id).await["workspace_id"].clone();
    assert_eq!(session_row(&mut client, &id).await["archived"], false);
    // In a section, then pinned: pinning leaves the section as it is.
    let section = client
        .result("section.create", json!({ "name": "Kept" }))
        .await;
    client
        .result(
            "session.arrange",
            json!({ "id": id, "section_id": section["id"] }),
        )
        .await;
    client
        .result("session.arrange", json!({ "id": id, "pinned": true }))
        .await;
    let folder = workspace_path(&mut client, &workspace_id).await;
    assert!(folder.is_dir(), "the workspace's folder exists before");

    let response = client
        .call(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;
    support::assert_valid("session.set_archived", &response);
    let archived = &response["result"];
    assert_eq!(archived["id"], id.as_str());
    assert_eq!(archived["archived"], true);
    assert_eq!(archived["state"], "ended");
    assert_eq!(stats.stops.load(Ordering::SeqCst), 1);
    let events = client.events_until("session.ended").await;
    assert_eq!(
        events.last().unwrap()["event"]["reason"],
        "user_stopped",
        "stopped as session.stop stops it"
    );

    // Still listed, archived, with its workspace.
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["archived"], true);
    assert_eq!(row["state"], "ended");
    assert_eq!(row["workspace_id"], workspace_id);
    assert!(workspace_listed(&mut client, &workspace_id).await);
    // Its pin and its section are kept, and nothing was removed.
    assert_eq!(row["pinned"], true);
    assert_eq!(row["section_id"], section["id"]);
    assert!(
        folder.is_dir(),
        "the archive removed the workspace's folder"
    );
}

/// Archiving is refused while the session is starting or a turn runs in it,
/// one waiting on the user included, and the refusal changes nothing: the
/// session is neither stopped nor marked archived.
// verifies: PROTO-CORE-API-R77
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R11
#[tokio::test(flavor = "multi_thread")]
async fn archiving_is_refused_while_the_session_is_starting_or_a_turn_runs() {
    /// Asserts `session.set_archived` refuses `id` as busy and leaves it be.
    async fn refused(client: &mut Client, stats: &Stats, id: &str, what: &str) {
        let response = client
            .call(
                "session.set_archived",
                json!({ "id": id, "archived": true }),
            )
            .await;
        support::assert_valid("session.set_archived", &response);
        let error = &response["error"];
        assert_eq!(kind(error), "invalid_state", "{what}: {response}");
        assert_eq!(
            error["data"]["reason"], "session_busy",
            "{what}: {response}"
        );
        let row = session_row(client, id).await;
        assert_eq!(row["archived"], false, "{what}: not archived");
        assert_ne!(row["state"], "ended", "{what}: not stopped");
        assert_eq!(stats.stops.load(Ordering::SeqCst), 0, "{what}: not stopped");
    }

    // Starting: the backend takes a while to become ready.
    let (core, stats) = start_fake(FakeAdapter::slow_start(Duration::from_secs(2))).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    refused(&mut client, &stats, &id, "starting").await;

    // A turn running, and one waiting on the user.
    for (text, what) in [("work", "running"), ("ask", "waiting on the user")] {
        let (core, stats) = start_fake(FakeAdapter::new(1)).await;
        let mut client = core.client().await;
        let id = new_session(&core, &mut client).await;
        client
            .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
            .await;
        client.events_until("session.started").await;
        client
            .result("session.send", json!({ "id": id, "text": text }))
            .await;
        if text == "ask" {
            client.events_until("permission.requested").await;
        } else {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            while session_row(&mut client, &id).await["state"] != "running" {
                assert!(tokio::time::Instant::now() < deadline, "never running");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        refused(&mut client, &stats, &id, what).await;
    }
}

/// What `session.set_archived` sets outlives Core, and `session.list`
/// reports it: `session.set_archived`'s half of R34 (`arrangement.rs` has
/// `session.arrange`'s).
// verifies: PROTO-CORE-API-R34
#[tokio::test(flavor = "multi_thread")]
async fn an_archived_session_is_still_archived_after_core_restarts() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let archived = idle_session(&core, &mut client).await;
    let workspace_id = session_row(&mut client, &archived).await["workspace_id"].clone();
    let kept = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result(
            "session.set_archived",
            json!({ "id": archived, "archived": true }),
        )
        .await;
    client.result("session.stop", json!({ "id": kept })).await;
    drop(client);

    let (_core, mut client) = restarted(core, FakeAdapter::new(1)).await;
    assert_eq!(session_row(&mut client, &archived).await["archived"], true);
    assert_eq!(session_row(&mut client, &kept).await["archived"], false);
}

/// Core refuses to send an archived session a message: it is read-only
/// until it is unarchived, or a resume is accepted.
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R02
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_message_to_an_archived_session_is_refused() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = idle_session(&core, &mut client).await;
    let sent = stats.messages.lock().unwrap().len();
    client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;

    let response = client
        .call("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    support::assert_valid("session.send", &response);
    let error = &response["error"];
    assert_eq!(kind(error), "invalid_state");
    assert_eq!(error["data"]["reason"], "session_archived", "{error:#}");
    // Refused because it is archived, not only because it has stopped.
    assert!(
        error["message"].as_str().unwrap().contains("archived"),
        "{error:#}"
    );
    assert_eq!(stats.messages.lock().unwrap().len(), sent);

    // Still refused after a restart, which kept the mark.
    drop(client);
    let (_core, mut client) = restarted(core, FakeAdapter::new(1)).await;
    let error = client
        .error("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
    assert!(
        error["message"].as_str().unwrap().contains("archived"),
        "{error:#}"
    );
}

/// A request that needs the backend running, to a session that has
/// stopped, is refused `session_not_running`.
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_request_to_a_stopped_session_says_it_is_not_running() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = idle_session(&core, &mut client).await;
    client.result("session.stop", json!({ "id": id })).await;
    let error = client.error("session.interrupt", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert_eq!(error["data"]["reason"], "session_not_running", "{error:#}");
    assert_eq!(error["message"], "the session is not running");
}

/// A backend that says it is not running, having exited before a write
/// while Core still held its run, is refused `session_not_running` too.
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_backend_that_exited_before_a_write_says_it_is_not_running() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = idle_session(&core, &mut client).await;
    let error = client
        .error(
            "session.send",
            json!({ "id": id, "text": "stopped before the write" }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert_eq!(error["data"]["reason"], "session_not_running", "{error:#}");
    assert_eq!(error["message"], "the session is not running");
}

/// A second message while one is still being sent is refused
/// `send_in_progress`; the first goes on.
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_message_while_another_is_being_sent_is_refused_send_in_progress() {
    let adapter = FakeAdapter::new(1);
    let start_gate = Arc::clone(&adapter.start_gate);
    let (core, _) = start_fake(adapter).await;
    // Held, the backend `session.create` starts is never ready, so the
    // first message waits for it holding the session's send slot for as
    // long as the test needs.
    let held = start_gate.lock().await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    let first = client
        .request("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    // Waited for, not raced: the two connections' sends are not ordered, so
    // the second is sent only once the first has the slot. Only the send
    // holding the slot reports `starting`; one still waiting for its turn
    // reports `checking`.
    client
        .notification_where(|n| {
            n["method"] == "session.send_progress"
                && n["params"]["session_id"] == id.as_str()
                && n["params"]["phase"] == "starting"
        })
        .await;
    let mut second = core.client().await;
    let error = second
        .error("session.send", json!({ "id": id, "text": "again" }))
        .await;
    assert_eq!(error["data"]["reason"], "send_in_progress", "{error:#}");
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("already being sent"),
        "{error:#}"
    );
    // The first goes on: once the backend is ready it has the message.
    drop(held);
    let first = client.response(first).await;
    assert!(first.get("result").is_some(), "{first:#}");
}

/// Renaming a stopped session whose backend has no record of it is refused
/// `record_gone`.
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn renaming_a_session_whose_record_is_gone_is_refused_record_gone() {
    let adapter = FakeAdapter::new(1);
    let records = Arc::clone(&adapter.records);
    let (core, _) = start_fake(adapter).await;
    let mut client = core.client().await;
    let id = idle_session(&core, &mut client).await;
    client.result("session.stop", json!({ "id": id })).await;
    records.lock().unwrap().clear();
    let error = client
        .error("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error:#}");
    assert_eq!(error["data"]["reason"], "record_gone", "{error:#}");
    assert!(
        error["message"].as_str().unwrap().contains("no record"),
        "{error:#}"
    );
}
/// Unarchiving clears the mark, for good, and leaves the session stopped;
/// a message is no longer refused for being archived.
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R03
#[tokio::test(flavor = "multi_thread")]
async fn unarchiving_clears_the_mark_and_leaves_the_session_stopped() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = idle_session(&core, &mut client).await;
    client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        )
        .await;
    let starts = stats.starts.load(Ordering::SeqCst);

    let unarchived = client
        .result(
            "session.set_archived",
            json!({ "id": id, "archived": false }),
        )
        .await;
    assert_eq!(unarchived["archived"], false);
    assert_eq!(unarchived["state"], "ended");
    assert_eq!(stats.starts.load(Ordering::SeqCst), starts, "not restarted");
    // A message is taken again: it resumes the session.
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    assert_eq!(stats.starts.load(Ordering::SeqCst), starts + 1);
    client.result("session.stop", json!({ "id": id })).await;

    drop(client);
    let (_core, mut client) = restarted(core, FakeAdapter::new(1)).await;
    assert_eq!(session_row(&mut client, &id).await["archived"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn archiving_an_unknown_session_is_not_found() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let error = client
        .error(
            "session.set_archived",
            json!({ "id": "s-missing", "archived": true }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");
}

// verifies: PROTO-CORE-API-R25
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupt_finds_a_send_handed_over_and_goes_on_to_the_backend() {
    let (core, stats) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = idle_session(&core, &mut client).await;
    let sent = stats.messages.lock().unwrap().len();
    let send = client
        .request(
            "session.send",
            json!({ "id": id, "text": "taken once interrupted" }),
        )
        .await;

    // The backend is taking the message: handed over, as the session lists
    // it, and past cancelling.
    let mut other = core.client().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while session_row(&mut other, &id).await.get("sending") != Some(&json!("handing_over")) {
        assert!(tokio::time::Instant::now() < deadline, "never handed over");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // So the interrupt does not stop the send: it is the turn's, and goes
    // on to the backend.
    other.result("session.interrupt", json!({ "id": id })).await;
    assert_eq!(stats.interrupts.load(Ordering::SeqCst), 1);
    let response = client.response(send).await;
    assert!(response.get("result").is_some(), "{response:#}");
    assert_eq!(stats.messages.lock().unwrap().len(), sent + 1);
}

// verifies: PROTO-CORE-API-R25
#[tokio::test(flavor = "multi_thread")]
async fn interrupt_ends_a_running_turn_as_interrupted_and_otherwise_does_nothing() {
    let (core, _) = start_fake(FakeAdapter::new(1)).await;
    let mut client = core.client().await;
    let id = new_session(&core, &mut client).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;

    // With no turn running, the interrupt succeeds and changes nothing.
    let idle = client
        .result("session.interrupt", json!({ "id": id }))
        .await;
    assert_eq!(idle["id"], id.as_str());

    // A turn that keeps working ends with outcome interrupted.
    client
        .result("session.send", json!({ "id": id, "text": "work" }))
        .await;
    client.events_until("turn.started").await;
    client
        .result("session.interrupt", json!({ "id": id }))
        .await;
    let events = client.events_until("turn.completed").await;
    let completed = &events.last().unwrap()["event"];
    assert_eq!(completed["outcome"], "interrupted");
    assert_eq!(completed["turn"], 1);
    // Only the one turn ended: the first interrupt ended nothing.
    let ended = events
        .iter()
        .filter(|e| e["event"]["type"] == "turn.completed")
        .count();
    assert_eq!(ended, 1);
}
