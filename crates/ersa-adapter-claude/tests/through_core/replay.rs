//! The replay server against a live Core: fixture 11 driven through Core
//! with the real Claude adapter and a stub CLI, then served by the replay
//! server, and the two compared (PROTO-CORE-API, Conformance).

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{Adapter, RawLine};
use ersa_core::projection::View;
use ersa_core::replay::Pace;
use serde_json::{Value, json};

use crate::claude::{
    FIXTURE, OnTransport, fixture, isolated_claude, parse, recording, start_replay_with, stub,
};
use crate::support::{Client, start_with_adapters, tempdir, workspace};

/// The one session a replay server serves.
async fn the_session(client: &mut Client) -> Value {
    let sessions = client.result("session.list", json!({})).await;
    let sessions = sessions.as_array().expect("a list");
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    sessions[0].clone()
}

/// An event envelope without what depends on the clock or on the ids Core
/// issues: what two Cores agree on for the same raw log.
fn comparable(mut envelope: Value) -> Value {
    let object = envelope.as_object_mut().expect("an envelope");
    object.remove("ts");
    object.remove("session_id");
    envelope
}

/// What a client saw of one session: its events and its `session.tile`
/// notifications, in the order they arrived.
#[derive(Default)]
struct Watched {
    events: Vec<Value>,
    tiles: Vec<Value>,
}

impl Watched {
    fn note(&mut self, notification: Value) {
        match notification["method"].as_str() {
            Some("session.event") => self.events.push(notification["params"]["event"].clone()),
            Some("session.tile") => self.tiles.push(notification["params"]["tile"].clone()),
            _ => {}
        }
    }

    /// Reads notifications until an event of type `event_type`.
    async fn until(&mut self, client: &mut Client, event_type: &str) {
        while self
            .events
            .last()
            .is_none_or(|e| e["event"]["type"] != event_type)
        {
            let notification = client
                .notification()
                .await
                .expect("the connection stays open");
            self.note(notification);
        }
    }

    /// Reads notifications until a tile equal to the one `session.list`
    /// reports arrives: every change is announced, so the stream ends there.
    async fn until_settled(&mut self, client: &mut Client) {
        let settled = the_session(client).await["tile"].clone();
        while self.tiles.last() != Some(&settled) {
            let notification = client
                .notification()
                .await
                .expect("the connection stays open");
            self.note(notification);
        }
    }
}

/// Fixture 11 driven through a live Core: the real Claude adapter against a
/// stub CLI, the prompt and the decision sent over the API.
async fn live_core_turn() -> Watched {
    let (stdout, sent) = fixture();
    let adapter = OnTransport {
        adapter: isolated_claude().with_stop_grace(Duration::from_secs(1)),
        transport: Arc::new(stub(&stdout)),
    };
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(adapter)];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": "claude-code" }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let prompt = parse(&sent[1])["message"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.send", json!({ "id": id, "text": prompt }))
        .await;
    let mut watched = Watched::default();
    watched.until(&mut client, "permission.requested").await;
    let request_id = watched.events.last().unwrap()["event"]["request_id"].clone();
    client
        .result(
            "session.decide_permission",
            json!({ "id": id, "request_id": request_id, "behavior": "allow" }),
        )
        .await;
    watched.until(&mut client, "turn.completed").await;
    watched.until_settled(&mut client).await;
    client.result("session.stop", json!({ "id": id })).await;
    watched
}

/// Drops repeats of the tile just before: tiles are coalesced to the newest
/// per session, so how many repeats a client sees depends on timing, not
/// on the session.
fn changes(tiles: &[Value]) -> Vec<&Value> {
    let mut changes: Vec<&Value> = Vec::new();
    for tile in tiles {
        if changes.last() != Some(&tile) {
            changes.push(tile);
        }
    }
    changes
}

/// Every tile fixture 11 passes through, projected one line at a time.
/// Core projects the lines queued at once together and a connection gets
/// only the newest tile, so a client sees some of these, in this order,
/// and which ones depends on timing, not on the Core.
fn tiles_line_by_line() -> Vec<Value> {
    let projector = isolated_claude().projector();
    let (mut view, _) = View::open("fixture", "claude-code", Some(projector), &[]);
    let mut tiles = vec![serde_json::to_value(view.tile()).unwrap()];
    for (index, recorded) in recording().into_lines().into_iter().enumerate() {
        view.live(&RawLine {
            index: index as u64,
            ts: "2026-01-01T00:00:00.000Z".to_string(),
            dir: recorded.dir,
            line: recorded.line,
        });
        tiles.push(serde_json::to_value(view.tile()).unwrap());
    }
    tiles
}

/// Asserts that every tile a client was sent is one the session passed
/// through, in the order it passed through them.
fn assert_passed_through(client: &str, tiles: &[Value], passed: &[Value]) {
    let mut rest = passed.iter();
    for tile in changes(tiles) {
        assert!(
            rest.any(|p| p == tile),
            "the {client} client was sent {tile:#}, which is not among the tiles \
             the session passes through, in order: {passed:#?}"
        );
    }
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn the_replay_server_sends_the_notifications_core_sends_for_the_same_session() {
    let live = live_core_turn().await;

    let replay = start_replay_with(|_| {}, Pace::Instant).await;
    let mut client = replay.client().await;
    let session = the_session(&mut client).await;
    assert_eq!(session["backend"], "claude-code");
    assert_eq!(session["title"], FIXTURE);
    let id = session["id"].clone();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let mut replayed = Watched::default();
    replayed.until(&mut client, "turn.completed").await;
    replayed.until_settled(&mut client).await;
    for event in &replayed.events {
        assert_eq!(event["session_id"], id);
    }

    // Up to the end of the turn: what follows it arrives while waiting for
    // the tile to settle, as much of it as the timing allows.
    let through_turn = |events: Vec<Value>| -> Vec<Value> {
        let end = events
            .iter()
            .position(|e| e["event"]["type"] == "turn.completed")
            .expect("the turn completed");
        events.into_iter().take(end + 1).map(comparable).collect()
    };
    let live_events = through_turn(live.events);
    let replayed_events = through_turn(replayed.events);
    assert_eq!(replayed_events, live_events);
    // The tiles the rail is sent: each Core sends tiles the session passes
    // through, in order, and both end on the same tile. Which tiles between
    // the first and the last a client sees depends on timing, so the two
    // are not compared to each other.
    let passed = tiles_line_by_line();
    assert_passed_through("live", &live.tiles, &passed);
    assert_passed_through("replay", &replayed.tiles, &passed);
    assert_eq!(live.tiles.last(), passed.last());
    assert_eq!(replayed.tiles.last(), live.tiles.last());
}
