//! A message sent while a turn runs (PROTO-CORE-API, Sending): handed over
//! at once and held by the backend, which takes it in when it next can; the
//! tile's `waiting` says how many it holds (PROTO-CORE-API-R81). Through the
//! Core API with the fake adapter (`support/fake.rs`), whose backend holds a
//! message sent while `work` runs until `take in`.

mod support;

use std::sync::Arc;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter};
use support::{Client, TestCore, kind, start_with_adapters, tempdir, workspace};

async fn start_fake() -> TestCore {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    start_with_adapters(tempdir(), |_| {}, adapters).await
}

/// A new session of the fake backend, subscribed, with a turn working.
async fn working_session(core: &TestCore, client: &mut Client) -> String {
    let workspace_id = workspace(client, core.dir.path()).await;
    let id = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "work" }))
        .await;
    client.events_until("turn.started").await;
    id
}

/// The next `session.tile` for `id` whose tile satisfies `wanted`.
async fn tile_where(client: &mut Client, id: &str, wanted: impl Fn(&Value) -> bool) -> Value {
    client
        .notification_where(|n| {
            n["method"] == "session.tile"
                && n["params"]["session_id"] == id
                && wanted(&n["params"]["tile"])
        })
        .await["params"]["tile"]
        .clone()
}

async fn listed_tile(client: &mut Client, id: &str) -> Value {
    let sessions = client.result("session.list", json!({})).await;
    sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .unwrap_or_else(|| panic!("session {id} listed"))["tile"]
        .clone()
}

/// Sent during a turn, a message is answered once the backend has it,
/// without a turn, since it is not projected until the backend takes it in;
/// the session can send again at once, and the tile counts what waits.
// verifies: PROTO-CORE-API-R81
#[tokio::test(flavor = "multi_thread")]
async fn messages_sent_during_a_turn_are_handed_over_and_counted_until_taken_in() {
    let core = start_fake().await;
    let mut client = core.client().await;
    let id = working_session(&core, &mut client).await;
    assert!(
        listed_tile(&mut client, &id).await.get("waiting").is_none(),
        "nothing waits yet"
    );

    let first = client
        .result("session.send", json!({ "id": id, "text": "one" }))
        .await;
    assert!(first.get("turn").is_none(), "not projected yet: {first:#}");
    let tile = tile_where(&mut client, &id, |t| t.get("waiting").is_some()).await;
    assert_eq!(tile["waiting"], 1);

    // The session can send again, and the second waits behind the first.
    let second = client
        .result("session.send", json!({ "id": id, "text": "two" }))
        .await;
    assert!(
        second.get("turn").is_none(),
        "not projected yet: {second:#}"
    );
    let tile = tile_where(&mut client, &id, |t| t["waiting"] == 2).await;
    assert_eq!(tile["waiting"], 2);
    assert_eq!(listed_tile(&mut client, &id).await["waiting"], 2);

    // The backend takes them in, joined, into the running turn: none waits.
    client
        .result("session.send", json!({ "id": id, "text": "take in" }))
        .await;
    let events = client.events_until("user.message").await;
    let message = &events.last().unwrap()["event"];
    assert_eq!(message["text"], "one\ntwo\ntake in");
    assert!(
        events.iter().all(|e| e["event"]["type"] != "turn.started"),
        "joined the running turn: {events:#?}"
    );
    // Listed, not awaited as a `session.tile`: that may have come before
    // the event, and `events_until` passes over it. The tile is updated
    // with the event, so the list already has it.
    let tile = listed_tile(&mut client, &id).await;
    assert!(tile.get("waiting").is_none(), "{tile:#}");
}

/// A session that is not live holds nothing: its tile drops the count when
/// it ends, though the backend held messages when it stopped.
// verifies: PROTO-CORE-API-R81
#[tokio::test(flavor = "multi_thread")]
async fn the_count_goes_when_the_session_ends() {
    let core = start_fake().await;
    let mut client = core.client().await;
    let id = working_session(&core, &mut client).await;
    client
        .result("session.send", json!({ "id": id, "text": "one" }))
        .await;
    tile_where(&mut client, &id, |t| t["waiting"] == 1).await;

    client.result("session.stop", json!({ "id": id })).await;
    let tile = tile_where(&mut client, &id, |t| t["state"] == "ended").await;
    assert!(tile.get("waiting").is_none(), "{tile:#}");
    assert!(listed_tile(&mut client, &id).await.get("waiting").is_none());
}

/// Whether a refused send wrote anything is judged from its own call: a
/// permission answer written while the backend refused the message does not
/// make the refusal `delivery_unknown`.
#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_that_wrote_nothing_is_not_sent_though_an_answer_was_written_meanwhile() {
    let core = start_fake().await;
    let mut client = core.client().await;
    let workspace_id = workspace(&mut client, core.dir.path()).await;
    let id = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "ask" }))
        .await;
    client.events_until("permission.requested").await;

    // The backend refuses this one only once the answer below is written.
    let send = client
        .request(
            "session.send",
            json!({ "id": id, "text": "refused once answered" }),
        )
        .await;
    client
        .result(
            "session.decide_permission",
            json!({ "id": id, "request_id": "r-1", "behavior": "allow" }),
        )
        .await;
    let answer = client.response(send).await;
    let error = &answer["error"];
    assert!(!error.is_null(), "{answer:#}");
    assert_ne!(kind(error), "delivery_unknown", "{answer:#}");
}
