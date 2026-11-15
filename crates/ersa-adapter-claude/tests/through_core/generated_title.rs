//! Naming a new session (PROTO-CLAUDE-CODE-STREAM, Naming a new session) and
//! the tiles that tell every client a session's title (PROTO-CORE-API-R83),
//! through Core's API with the real Claude adapter and a stub CLI replaying
//! fixture 2.1.316/43-session-title: the first message, the request to name
//! the session, its answer before the message's echo, then the turn.

use std::time::Duration;

use crate::claude::{fixtures_of, parse};
use crate::naming::{claude_core, create, is, listed_title};
use crate::support::{Client, TestCore, tempdir};
use ersa_adapter::fake::FakeTransport;
use ersa_adapter::recording::Recording;
use serde_json::{Value, json};

/// The name the recorded CLI generated.
const GENERATED: &str = "Why the sky is blue";
/// A name the user gives the session.
const MINE: &str = "Mine";

/// Fixture 43's stdout up to the end of the first turn, with the answer to
/// the title request given the id the driver gives that request
/// (`ersa-title-2`, its first after `initialize`), and its `response`
/// replaced by `response` when one is given.
fn first_turn(response: Option<Value>) -> Vec<String> {
    let stdout = Recording::load(&fixtures_of("2.1.316").join("43-session-title.jsonl"))
        .expect("fixture loads")
        .stdout();
    let end = stdout
        .iter()
        .position(|l| parse(l)["type"] == "result")
        .expect("the first turn ends");
    stdout[..=end]
        .iter()
        .map(|line| {
            let mut v = parse(line);
            if v["response"]["request_id"] == "ersa-generate_session_title-1" {
                v["response"]["request_id"] = json!("ersa-title-2");
                if let Some(response) = &response {
                    v["response"] = response.clone();
                }
            }
            v.to_string()
        })
        .collect()
}

/// Splits `stdout` at the answer to the title request: what comes before
/// it, the answer, and what follows.
fn around_answer(stdout: &[String]) -> (Vec<String>, String, Vec<String>) {
    let at = stdout
        .iter()
        .position(|l| parse(l)["response"]["request_id"] == "ersa-title-2")
        .expect("the answer");
    (
        stdout[..at].to_vec(),
        stdout[at].clone(),
        stdout[at + 1..].to_vec(),
    )
}

/// The CLI's announcement of the user's name, then its answer to the
/// rename, which the driver numbers 3 (after `initialize` and the title).
fn renamed(uuid: &str) -> [String; 2] {
    [
        json!({"type": "system", "subtype": "session_title_changed", "title": MINE,
            "session_id": "39885328-1e47-4d9e-804e-5ec43fc6efb1", "uuid": uuid})
        .to_string(),
        json!({"type": "control_response",
            "response": {"subtype": "success", "request_id": "ersa-rename-3"}})
        .to_string(),
    ]
}

/// A stub CLI that answers `initialize`, takes the first message and the
/// request to name the session, and plays `stdout`'s first turn.
fn naming_cli(stdout: &[String]) -> FakeTransport {
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(is("initialize"))
        .then_stdout([&stdout[0]])
        .wait_for_stdin(is("user"))
        .wait_for_stdin(is("generate_session_title"))
        .then_stdout(&stdout[1..])
}

/// Reads notifications until a `session.tile` for `id` whose `title` is
/// `title` (`Value::Null` for none), failing after 10 seconds.
async fn tile_titled(client: &mut Client, id: &str, title: Value) -> Value {
    let wanted = |n: &Value| {
        n["method"] == "session.tile"
            && n["params"]["session_id"] == id
            && n["params"].get("title").cloned().unwrap_or(Value::Null) == title
    };
    tokio::time::timeout(Duration::from_secs(10), client.notification_where(wanted))
        .await
        .unwrap_or_else(|_| panic!("no tile titled {title} within 10 s"))["params"]
        .clone()
}

/// Creates a session, subscribes, and sends fixture 43's first message.
async fn first_message(core: &TestCore, client: &mut Client) -> String {
    let id = create(core, client).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client
        .result(
            "session.send",
            json!({ "id": id, "text": "Why is the sky blue? Answer in one sentence." }),
        )
        .await;
    id
}

fn no_stub_failures(transport: &FakeTransport) {
    assert!(
        transport.stub_failures().is_empty(),
        "{:?}",
        transport.stub_failures()
    );
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R50
// verifies: PROTO-CORE-API-R83
// verifies: PROTO-CORE-API-R84
#[tokio::test(flavor = "multi_thread")]
async fn the_generated_name_is_the_title_and_a_tile_carries_it() {
    let transport = naming_cli(&first_turn(None)).hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    // A second window, which renames nothing and reads no list.
    let mut other = core.client().await;
    let id = first_message(&core, &mut client).await;

    let tile = tile_titled(&mut other, &id, json!(GENERATED)).await;
    assert!(tile.get("tile").is_some(), "{tile:#}");
    assert_eq!(listed_title(&mut client, &id).await, GENERATED);
    // The turn's end sends a tile, which carries the title too: every
    // tile does.
    let events = client.events_until("turn.completed").await;
    let mut ended = tile;
    while ended.get("last_activity_at").is_none() {
        ended = tile_titled(&mut other, &id, json!(GENERATED)).await;
    }
    assert!(
        events.iter().all(|e| e["event"]["type"] != "error"),
        "{events:#?}"
    );
    client.result("session.stop", json!({ "id": id })).await;
    no_stub_failures(&transport);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R50
// verifies: PROTO-CORE-API-R83
#[tokio::test(flavor = "multi_thread")]
async fn a_rename_before_the_answer_wins_over_the_generated_name() {
    let (before, answer, after) = around_answer(&first_turn(None));
    let transport = naming_cli(&before)
        .wait_for_stdin(is("rename_session"))
        .then_stdout(renamed("00000000-0000-4000-8000-000000000001"))
        // The CLI's own name comes after the user's.
        .then_stdout([answer])
        .then_stdout(&after)
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let mut other = core.client().await;
    let id = first_message(&core, &mut client).await;
    let shown = client
        .result("session.set_title", json!({ "id": id, "title": MINE }))
        .await;
    assert_eq!(shown["title"], MINE);
    // The other window hears of the rename.
    tile_titled(&mut other, &id, json!(MINE)).await;
    client.events_until("turn.completed").await;
    assert_eq!(listed_title(&mut client, &id).await, MINE);
    client.result("session.stop", json!({ "id": id })).await;
    no_stub_failures(&transport);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R50
// verifies: PROTO-CORE-API-R83
#[tokio::test(flavor = "multi_thread")]
async fn a_rename_after_the_generated_name_replaces_it_on_every_window() {
    let transport = naming_cli(&first_turn(None))
        .wait_for_stdin(is("rename_session"))
        .then_stdout(renamed("00000000-0000-4000-8000-000000000002"))
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let mut other = core.client().await;
    let id = first_message(&core, &mut client).await;
    client.events_until("turn.completed").await;
    tile_titled(&mut other, &id, json!(GENERATED)).await;

    client
        .result("session.set_title", json!({ "id": id, "title": MINE }))
        .await;
    tile_titled(&mut other, &id, json!(MINE)).await;
    assert_eq!(listed_title(&mut client, &id).await, MINE);
    client.result("session.stop", json!({ "id": id })).await;
    no_stub_failures(&transport);
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R50
#[tokio::test(flavor = "multi_thread")]
async fn an_error_answer_names_nothing_and_tells_the_user_nothing() {
    let error = json!({"subtype": "error", "request_id": "ersa-title-2",
        "error": "Unsupported control request subtype: generate_session_title"});
    let transport = naming_cli(&first_turn(Some(error))).hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let id = first_message(&core, &mut client).await;
    let events = client.events_until("turn.completed").await;
    assert_eq!(events.last().unwrap()["event"]["outcome"], "success");
    assert!(
        events
            .iter()
            .all(|e| e["event"]["type"] != "error" && e["event"]["type"] != "session.notice"),
        "{events:#?}"
    );
    // No name: the stub CLI writes no record to take one from.
    assert_eq!(listed_title(&mut client, &id).await, Value::Null);
    client.result("session.stop", json!({ "id": id })).await;
    no_stub_failures(&transport);
}

// verifies: PROTO-CORE-API-R83
#[tokio::test(flavor = "multi_thread")]
async fn a_name_the_cli_announces_on_its_own_sends_a_tile() {
    // A rename made in the CLI itself, after the turn: nothing else about
    // the session changes with it.
    let [announcement, _] = renamed("00000000-0000-4000-8000-000000000003");
    let transport = naming_cli(&first_turn(None))
        .then_pause(Duration::from_millis(300))
        .then_stdout([announcement])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let mut other = core.client().await;
    let id = first_message(&core, &mut client).await;
    tile_titled(&mut other, &id, json!(MINE)).await;
    client.result("session.stop", json!({ "id": id })).await;
    no_stub_failures(&transport);
}

// verifies: PROTO-CORE-API-R83
#[tokio::test(flavor = "multi_thread")]
async fn a_live_rename_sends_a_tile_though_the_cli_announces_nothing() {
    let [_, answer] = renamed("unused");
    let transport = naming_cli(&first_turn(None))
        .wait_for_stdin(is("rename_session"))
        .then_stdout([answer])
        .hold_open();
    let home = tempdir();
    let core = claude_core(&transport, home.path()).await;
    let mut client = core.client().await;
    let mut other = core.client().await;
    let id = first_message(&core, &mut client).await;
    client.events_until("turn.completed").await;
    tile_titled(&mut other, &id, json!(GENERATED)).await;
    client
        .result("session.set_title", json!({ "id": id, "title": MINE }))
        .await;
    tile_titled(&mut other, &id, json!(MINE)).await;
    client.result("session.stop", json!({ "id": id })).await;
    no_stub_failures(&transport);
}
