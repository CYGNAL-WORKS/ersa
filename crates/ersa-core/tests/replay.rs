//! The replay server: Core serving a recorded session over the real socket,
//! with no backend behind it (ARCH-CORE, Implementation decisions;
//! PROTO-CORE-API, Conformance).

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{Adapter, Direction, RawLine};
use ersa_core::projection::View;
use ersa_core::replay::Pace;
use serde_json::{Value, json};
use support::claude::{
    FIXTURE, OnTransport, fixture, isolated_claude, parse, recording, start_replay_with, stub,
};
use support::suite::{self, Target};
use support::{Client, TestCore, kind, start_with_adapters, tempdir, workspace};

async fn start_replay(pace: Pace) -> TestCore {
    start_replay_with(|_| {}, pace).await
}

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

/// Waits until the replay has recorded every line of the recording: lines
/// follow the end of the turn, so `turn.completed` alone does not say so.
async fn played_through(client: &mut Client, id: &Value) {
    let lines = recording().lines().len();
    for _ in 0..600 {
        let log = client.result("session.raw_log", json!({ "id": id })).await;
        let crossed = log.as_array().unwrap().len();
        if crossed == lines {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the replay did not play its {lines} lines within 30 s");
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn the_replay_server_sends_the_notifications_core_sends_for_the_same_session() {
    let live = live_core_turn().await;

    let replay = start_replay(Pace::Instant).await;
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

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn read_only_requests_are_answered_as_core_answers_them() {
    let replay = start_replay(Pace::Instant).await;
    let mut client = replay.client().await;

    let session = the_session(&mut client).await;
    let id = session["id"].clone();
    let status = client.result("core.status", json!({})).await;
    assert_eq!(status["sessions_running"], 1);
    let hosts = client.result("host.list", json!({})).await;
    assert_eq!(hosts[0]["id"], "local");
    client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    let projects = client.result("project.list", json!({})).await;
    assert_eq!(projects.as_array().unwrap().len(), 1);
    let workspaces = client
        .result("workspace.list", json!({ "project_id": projects[0]["id"] }))
        .await;
    assert_eq!(workspaces[0]["id"], session["workspace_id"]);
    let found = client
        .result(
            "project.find",
            json!({ "host_id": "local", "path": projects[0]["path"] }),
        )
        .await;
    assert_eq!(found, projects[0]);
    // The recording's project has no version control, so Core answers as it
    // does for any such project, rather than as a replay refusing a change.
    let error = client
        .error("branch.list", json!({ "project_id": projects[0]["id"] }))
        .await;
    assert_eq!(error["data"]["kind"], "invalid_state");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("workspace adapter"),
        "{error}"
    );
    assert_eq!(client.result("settings.get", json!({})).await, json!({}));

    // Nothing is played until a client subscribes.
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    assert_eq!(log, json!([]));

    // Once the whole recording is in, the raw log holds it line for line.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    played_through(&mut client, &id).await;
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let crossed: Vec<(String, String)> = log
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["dir"].as_str().unwrap().to_string(),
                e["line"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let recorded: Vec<(String, String)> = recording()
        .lines()
        .iter()
        .map(|l| {
            let dir = match l.dir {
                Direction::Backend => "backend",
                Direction::Host => "host",
            };
            (dir.to_string(), l.line.clone())
        })
        .collect();
    assert_eq!(crossed, recorded);

    // core.shutdown stops the replay server as it stops Core.
    client.result("core.shutdown", json!({})).await;
    tokio::time::timeout(Duration::from_secs(30), replay.task)
        .await
        .expect("the server stops")
        .expect("no panic")
        .expect("no error");
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_that_would_change_anything_answer_invalid_state_naming_the_replay() {
    let replay = start_replay(Pace::Instant).await;
    let mut client = replay.client().await;
    let id = the_session(&mut client).await["id"].clone();
    // Once the whole recording is in, so nothing changes by itself.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    played_through(&mut client, &id).await;
    let session = the_session(&mut client).await;
    let workspace_id = session["workspace_id"].clone();

    let refused = [
        (
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": "claude-code" }),
        ),
        ("session.start", json!({ "id": id })),
        ("session.stop", json!({ "id": id })),
        ("session.resume", json!({ "id": id })),
        ("session.remove", json!({ "id": id })),
        ("session.send", json!({ "id": id, "text": "hello" })),
        ("session.interrupt", json!({ "id": id })),
        (
            "session.decide_permission",
            json!({ "id": id, "request_id": "r", "behavior": "allow" }),
        ),
        (
            "session.answer_question",
            json!({ "id": id, "request_id": "r", "answers": {} }),
        ),
        ("session.set_title", json!({ "id": id, "title": "t" })),
        ("session.set_mode", json!({ "id": id, "mode": "m" })),
        (
            "project.add",
            json!({ "host_id": "local", "path": ".", "trusted": false }),
        ),
        ("project.set_trust", json!({ "id": "p", "trusted": true })),
        ("project.remove", json!({ "id": "p" })),
        ("workspace.create", json!({ "project_id": "p" })),
        ("workspace.remove", json!({ "id": workspace_id })),
        (
            "settings.set",
            json!({ "patch": { "workspaces": { "root": "." } } }),
        ),
    ];
    for (method, params) in refused {
        let error = client.error(method, params).await;
        assert_eq!(kind(&error), "invalid_state", "{method}: {error:#}");
        assert!(
            error["message"].as_str().unwrap().contains("replay"),
            "{method}: {error:#}"
        );
    }
    // Nothing changed.
    let after = the_session(&mut client).await;
    assert_eq!(after["title"], session["title"]);
    assert_eq!(after["state"], session["state"]);
}

// The shared request/response suite Core passes (`core_api.rs`,
// `connection_limits.rs`), run on the replay server.

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_every_method_before_hello_is_refused_with_invalid_state() {
    suite::every_method_before_hello_is_refused_with_invalid_state(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_hello_reports_versions_and_the_local_host() {
    suite::hello_reports_versions_and_the_local_host(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_hello_without_a_shared_version_names_the_supported_ones_and_closes() {
    suite::hello_without_a_shared_version_names_the_supported_ones_and_closes(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_reserved_methods_answer_invalid_state_naming_their_milestone() {
    suite::reserved_methods_answer_invalid_state_naming_their_milestone(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_protocol_errors_use_the_standard_codes() {
    suite::protocol_errors_use_the_standard_codes(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_an_unknown_session_is_not_found() {
    suite::an_unknown_session_is_not_found(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_client_notifications_get_no_answer() {
    suite::client_notifications_get_no_answer(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_pipelined_requests_are_answered_in_order() {
    suite::pipelined_requests_are_answered_in_order(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_a_line_over_the_cap_gets_a_parse_error_and_closes() {
    suite::a_line_over_the_cap_gets_a_parse_error_and_closes(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_a_line_that_is_not_utf8_gets_a_parse_error_and_closes() {
    suite::a_line_that_is_not_utf8_gets_a_parse_error_and_closes(Target::Replay).await;
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn suite_a_connection_without_hello_is_closed_after_the_deadline() {
    suite::a_connection_without_hello_is_closed_after_the_deadline(Target::Replay).await;
}
