//! The replay server: Core serving a recorded session over the real socket,
//! with no backend behind it (ARCH-CORE, Implementation decisions).

mod support;

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::recording::Recording;
use ersa_adapter::{Adapter, Direction};
use ersa_adapter_claude::ClaudeAdapter;
use ersa_core::Server;
use ersa_core::replay::{Pace, Replay};
use serde_json::{Value, json};
use support::claude::{FIXTURE, OnTransport, fixture, fixture_dir, parse, stub};
use support::{
    Client, TestCore, kind, start_with_adapters, tempdir, test_config, unique_endpoint, workspace,
};

fn recording() -> Recording {
    Recording::load(&fixture_dir().join(format!("{FIXTURE}.jsonl"))).expect("fixture loads")
}

/// A replay server for fixture 11, in a temporary data directory.
async fn start_replay(pace: Pace) -> TestCore {
    let dir = tempdir();
    let endpoint = unique_endpoint();
    let config = test_config(&dir.path().join("data"), &endpoint);
    let replay = Replay {
        adapter: Arc::new(ClaudeAdapter::new()),
        recording: recording(),
        title: FIXTURE.to_string(),
        // What detection finds in these tests' Cores, which have no backend
        // installed; the live Core below starts its session with it too.
        backend_version: String::new(),
        pace,
    };
    let server = Server::bind_replay(config, replay).await.expect("bind");
    let task = tokio::spawn(server.run());
    TestCore {
        dir,
        endpoint,
        task,
    }
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

/// Fixture 11 driven through a live Core: the real Claude adapter against a
/// stub CLI, the prompt and the decision sent over the API. The events up
/// to the end of the turn, and the session's tile then.
async fn live_core_events() -> (Vec<Value>, Value) {
    let (stdout, sent) = fixture();
    let adapter = OnTransport {
        adapter: ClaudeAdapter::new().with_stop_grace(Duration::from_secs(1)),
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
    let mut events = client.events_until("permission.requested").await;
    let request_id = events.last().unwrap()["event"]["request_id"].clone();
    client
        .result(
            "session.decide_permission",
            json!({ "id": id, "request_id": request_id, "behavior": "allow" }),
        )
        .await;
    events.extend(client.events_until("turn.completed").await);
    let tile = the_session(&mut client).await["tile"].clone();
    client.result("session.stop", json!({ "id": id })).await;
    (events, tile)
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn the_replay_server_sends_the_notifications_core_sends_for_the_same_session() {
    let (live, live_tile) = live_core_events().await;

    let replay = start_replay(Pace::Instant).await;
    let mut client = replay.client().await;
    let session = the_session(&mut client).await;
    assert_eq!(session["backend"], "claude-code");
    assert_eq!(session["title"], FIXTURE);
    let id = session["id"].clone();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let replayed = client.events_until("turn.completed").await;
    for event in &replayed {
        assert_eq!(event["session_id"], id);
    }

    let live: Vec<Value> = live.into_iter().map(comparable).collect();
    let replayed: Vec<Value> = replayed.into_iter().map(comparable).collect();
    assert_eq!(replayed, live);
    // And the rail shows the same tile.
    assert_eq!(the_session(&mut client).await["tile"], live_tile);
}

// verifies: PROTO-CORE-API-R09
#[tokio::test(flavor = "multi_thread")]
async fn read_only_requests_are_answered_as_core_answers_them() {
    let replay = start_replay(Pace::Instant).await;
    let mut client = Client::connect(&replay.endpoint).await;
    // The handshake comes first, as on Core (PROTO-CORE-API-R01).
    let error = client.error("session.list", json!({})).await;
    assert_eq!(kind(&error), "invalid_state");
    let hello = client.hello().await;
    assert_eq!(hello["api_version"], 1);

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
    assert_eq!(client.result("settings.get", json!({})).await, json!({}));
    // Reserved methods answer as on Core (PROTO-CORE-API-R10).
    let error = client.error("mode.list", json!({})).await;
    assert!(
        error["message"].as_str().unwrap().contains("M3"),
        "{error:#}"
    );

    // Once the whole recording is in, the raw log holds it line for line,
    // between Core's own records.
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("turn.completed").await;
    let log = client.result("session.raw_log", json!({ "id": id })).await;
    let crossed: Vec<(String, String)> = log
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["dir"] != "core")
        .map(|e| {
            (
                e["dir"].as_str().unwrap().to_string(),
                e["line"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let recorded: Vec<(String, String)> = recording()
        .timed_lines()
        .unwrap()
        .into_iter()
        .map(|(_, dir, line)| {
            let dir = match dir {
                Direction::Backend => "backend",
                Direction::Host => "host",
                Direction::Core => "core",
            };
            (dir.to_string(), line)
        })
        .collect();
    assert_eq!(crossed, recorded);
    assert_eq!(log[0]["dir"], "core");

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
    client.events_until("turn.completed").await;
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
        ("session.reproject", json!({ "id": id })),
        ("project.add", json!({ "host_id": "local", "path": "." })),
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
