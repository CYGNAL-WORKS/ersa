//! The user's arrangement of sessions (ADR 0026; PROTO-CORE-API, Sessions
//! and Sections; FLOW-ORGANIZE-SESSIONS), driven over the socket with the
//! fake backend: pins and sections set by `session.arrange`, the sections
//! themselves, and `last_activity_at`, each kept across a restart of Core.

mod support;

use std::sync::Arc;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter};
use support::{Client, TestCore, assert_valid, kind, start_with_adapters, tempdir, workspace};

async fn start() -> (TestCore, Client) {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let client = core.client().await;
    (core, client)
}

/// Restarts `core` on the same data directory, as a user closing and
/// opening Ersa would: the new Core knows only what its store kept.
async fn restarted(core: TestCore) -> (TestCore, Client) {
    let mut client = core.client().await;
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let client = core.client().await;
    (core, client)
}

/// A new session of the fake backend in workspace `workspace_id`.
async fn new_session(client: &mut Client, workspace_id: &Value) -> String {
    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": fake::BACKEND }),
        )
        .await["id"]
        .as_str()
        .unwrap()
        .to_string()
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

/// `session.arrange`, checked against the schema; returns its result.
async fn arrange(client: &mut Client, params: Value) -> Value {
    let response = client.call("session.arrange", params).await;
    assert_valid("session.arrange", &response);
    response["result"].clone()
}

/// `section.create`, checked against the schema; returns the section.
async fn create_section(client: &mut Client, name: &str) -> Value {
    let response = client.call("section.create", json!({ "name": name })).await;
    assert_valid("section.create", &response);
    response["result"].clone()
}

async fn sections(client: &mut Client) -> Value {
    let response = client.call("section.list", json!({})).await;
    assert_valid("section.list", &response);
    response["result"].clone()
}

/// The names `section.list` lists, in its order.
async fn section_names(client: &mut Client) -> Vec<String> {
    sections(client)
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect()
}

/// `session.arrange` changes the fields it is given and leaves the others;
/// a session belongs to one section at most; and what it sets, and the
/// sections themselves, outlive Core.
// verifies: PROTO-CORE-API-R34
// verifies: FLOW-ORGANIZE-SESSIONS-R03
// verifies: FLOW-ORGANIZE-SESSIONS-R13
#[tokio::test(flavor = "multi_thread")]
async fn arrange_sets_only_the_fields_given_and_each_outlives_core() {
    let (core, mut client) = start().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    let other = new_session(&mut client, &workspace_id).await;

    // A new session has none of the arrangement set.
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["pinned"], false);
    assert!(row.get("section_id").is_none(), "{row:#}");

    let work = create_section(&mut client, "Work").await;
    let later = create_section(&mut client, "Later").await;

    // Pinned; the section is left as it was (none).
    let arranged = arrange(&mut client, json!({ "id": id, "pinned": true })).await;
    assert_eq!(arranged["id"], id.as_str());
    assert_eq!(arranged["pinned"], true);
    assert!(arranged.get("section_id").is_none(), "{arranged:#}");

    // Moved to a section; the pin is left as it was.
    let arranged = arrange(&mut client, json!({ "id": id, "section_id": work["id"] })).await;
    assert_eq!(arranged["pinned"], true);
    assert_eq!(arranged["section_id"], work["id"]);

    // Moved to another section: in that one only.
    let arranged = arrange(&mut client, json!({ "id": id, "section_id": later["id"] })).await;
    assert_eq!(arranged["section_id"], later["id"]);

    // Unpinned; the section is left as it was.
    let arranged = arrange(&mut client, json!({ "id": id, "pinned": false })).await;
    assert_eq!(arranged["pinned"], false);
    assert_eq!(arranged["section_id"], later["id"]);

    // Both at once, on another session; the first is untouched.
    arrange(
        &mut client,
        json!({ "id": other, "pinned": true, "section_id": work["id"] }),
    )
    .await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["pinned"], false);
    assert_eq!(row["section_id"], later["id"]);

    drop(client);
    let (core, mut client) = restarted(core).await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["pinned"], false);
    assert_eq!(row["section_id"], later["id"]);
    let row = session_row(&mut client, &other).await;
    assert_eq!(row["pinned"], true);
    assert_eq!(row["section_id"], work["id"]);
    // The sections outlive Core too, in the order they were made.
    assert_eq!(sections(&mut client).await, json!([work, later]));

    // `null` is no section; the pin is left as it was.
    let arranged = arrange(&mut client, json!({ "id": other, "section_id": null })).await;
    assert!(arranged.get("section_id").is_none(), "{arranged:#}");
    assert_eq!(arranged["pinned"], true);
    // Given nothing, it changes nothing.
    let arranged = arrange(&mut client, json!({ "id": other })).await;
    assert_eq!(arranged["pinned"], true);
    assert!(arranged.get("section_id").is_none(), "{arranged:#}");

    drop(client);
    let (_core, mut client) = restarted(core).await;
    let row = session_row(&mut client, &other).await;
    assert_eq!(row["pinned"], true);
    assert!(row.get("section_id").is_none(), "{row:#}");
}

/// A section that does not exist is `not_found`, and the session is left
/// as it was, the pin given beside it included.
// verifies: PROTO-CORE-API-R34
#[tokio::test(flavor = "multi_thread")]
async fn arranging_into_a_missing_section_is_not_found_and_changes_nothing() {
    let (core, mut client) = start().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    let work = create_section(&mut client, "Work").await;
    arrange(&mut client, json!({ "id": id, "section_id": work["id"] })).await;

    let response = client
        .call(
            "session.arrange",
            json!({ "id": id, "pinned": true, "section_id": "sec-missing" }),
        )
        .await;
    assert_valid("session.arrange", &response);
    assert_eq!(kind(&response["error"]), "not_found", "{response:#}");
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["pinned"], false);
    assert_eq!(row["section_id"], work["id"]);

    let error = client
        .error(
            "session.arrange",
            json!({ "id": "s-missing", "pinned": true }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");
}

/// Core trims a section's name and refuses, with `invalid_params`, one
/// that is then blank, longer than 80 characters, or another section's,
/// ignoring case; `section.rename` likewise. A refusal changes nothing.
// verifies: FLOW-ORGANIZE-SESSIONS-R23
// verifies: PROTO-CORE-API-R70
#[tokio::test(flavor = "multi_thread")]
async fn section_names_are_trimmed_and_blank_long_or_taken_ones_refused() {
    let (core, mut client) = start().await;

    let work = create_section(&mut client, "  Work \t").await;
    assert_eq!(work["name"], "Work");
    assert!(work["id"].as_str().is_some_and(|id| !id.is_empty()));
    // 80 characters, not bytes: each of these is two bytes in UTF-8.
    let longest = "é".repeat(80);
    let summer = create_section(&mut client, &longest).await;
    assert_eq!(summer["name"], longest.as_str());

    let refused_names = [
        String::new(),
        " \t ".to_string(),
        "é".repeat(81),
        "x".repeat(81),
        "work".to_string(),
        " WORK ".to_string(),
        // Case is ignored beyond ASCII too.
        "É".repeat(80),
    ];
    for name in &refused_names {
        let response = client.call("section.create", json!({ "name": name })).await;
        assert_valid("section.create", &response);
        assert_eq!(response["error"]["code"], -32602, "{name:?}: {response:#}");
    }
    assert_eq!(
        section_names(&mut client).await,
        vec!["Work".to_string(), longest.clone()]
    );

    // Renaming: the same rules, and a section may keep its own name in
    // another case.
    for name in &refused_names {
        if name.trim().eq_ignore_ascii_case("work") {
            continue;
        }
        let response = client
            .call("section.rename", json!({ "id": work["id"], "name": name }))
            .await;
        assert_valid("section.rename", &response);
        assert_eq!(response["error"]["code"], -32602, "{name:?}: {response:#}");
    }
    let response = client
        .call(
            "section.rename",
            json!({ "id": summer["id"], "name": "work" }),
        )
        .await;
    assert_eq!(response["error"]["code"], -32602, "{response:#}");
    assert_eq!(
        section_names(&mut client).await,
        vec!["Work".to_string(), longest.clone()]
    );

    let response = client
        .call(
            "section.rename",
            json!({ "id": work["id"], "name": " WORK " }),
        )
        .await;
    assert_valid("section.rename", &response);
    assert_eq!(response["result"]["name"], "WORK");
    assert_eq!(response["result"]["id"], work["id"]);
    assert_eq!(response["result"]["created_at"], work["created_at"]);

    let error = client
        .error(
            "section.rename",
            json!({ "id": "sec-missing", "name": "x" }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");

    // A rename outlives Core.
    drop(client);
    let (_core, mut client) = restarted(core).await;
    assert_eq!(
        section_names(&mut client).await,
        vec!["WORK".to_string(), longest]
    );
}

/// `section.remove` puts the sessions it held in no section, keeping every
/// session, and that outlives Core.
// verifies: PROTO-CORE-API-R39
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_section_leaves_its_sessions_in_no_section() {
    let (core, mut client) = start().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let held = new_session(&mut client, &workspace_id).await;
    let pinned = new_session(&mut client, &workspace_id).await;
    let elsewhere = new_session(&mut client, &workspace_id).await;
    let work = create_section(&mut client, "Work").await;
    let later = create_section(&mut client, "Later").await;
    arrange(&mut client, json!({ "id": held, "section_id": work["id"] })).await;
    arrange(
        &mut client,
        json!({ "id": pinned, "section_id": work["id"], "pinned": true }),
    )
    .await;
    arrange(
        &mut client,
        json!({ "id": elsewhere, "section_id": later["id"] }),
    )
    .await;

    let response = client
        .call("section.remove", json!({ "id": work["id"] }))
        .await;
    assert_valid("section.remove", &response);
    assert_eq!(response["result"], work);

    let check = |row: Value| row.get("section_id").cloned();
    assert_eq!(check(session_row(&mut client, &held).await), None);
    let row = session_row(&mut client, &pinned).await;
    assert_eq!(check(row.clone()), None);
    assert_eq!(row["pinned"], true, "only its section changes");
    assert_eq!(
        check(session_row(&mut client, &elsewhere).await),
        Some(later["id"].clone())
    );
    assert_eq!(sections(&mut client).await, json!([later]));

    // Gone: not found, and its name is free again.
    let error = client
        .error("section.remove", json!({ "id": work["id"] }))
        .await;
    assert_eq!(kind(&error), "not_found");
    let error = client
        .error(
            "session.arrange",
            json!({ "id": held, "section_id": work["id"] }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");
    create_section(&mut client, "work").await;

    drop(client);
    let (_core, mut client) = restarted(core).await;
    assert_eq!(check(session_row(&mut client, &held).await), None);
    assert_eq!(check(session_row(&mut client, &pinned).await), None);
    assert_eq!(
        check(session_row(&mut client, &elsewhere).await),
        Some(later["id"].clone())
    );
}

/// `last_activity_at` is the time a session's latest turn ended: absent
/// until one has, set as each ends (whatever its outcome), and left alone
/// while a turn runs and a reply streams, and by anything else.
// verifies: PROTO-CORE-API-R69
#[tokio::test(flavor = "multi_thread")]
async fn last_activity_changes_only_when_a_turn_ends() {
    let (core, mut client) = start().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    let last_activity = |row: &Value| row.get("last_activity_at").cloned();
    assert_eq!(last_activity(&session_row(&mut client, &id).await), None);

    // A turn that keeps working: none yet.
    client
        .result("session.send", json!({ "id": id, "text": "work" }))
        .await;
    client.events_until("turn.started").await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["state"], "running");
    assert_eq!(last_activity(&row), None);

    // It ends, interrupted: the time it ended.
    client
        .result("session.interrupt", json!({ "id": id }))
        .await;
    let ended = client.events_until("turn.completed").await;
    let first = ended.last().unwrap()["ts"].clone();
    assert!(first.is_string());
    let row = session_row(&mut client, &id).await;
    assert_eq!(last_activity(&row), Some(first.clone()));
    let response = client.call("session.list", json!({})).await;
    assert_valid("session.list", &response);

    // A turn whose reply streams but has not ended: unchanged.
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.events_until("assistant.text").await;
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(first.clone())
    );

    // A turn that ends successfully: the time it ended.
    client
        .result("session.send", json!({ "id": id, "text": "finish" }))
        .await;
    let ended = client.events_until("turn.completed").await;
    let second = ended.last().unwrap()["ts"].clone();
    assert_eq!(ended.last().unwrap()["event"]["outcome"], "success");
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(second.clone())
    );

    // Arranging, renaming and stopping it are not turns.
    let work = create_section(&mut client, "Work").await;
    let arranged = arrange(
        &mut client,
        json!({ "id": id, "pinned": true, "section_id": work["id"] }),
    )
    .await;
    assert_eq!(last_activity(&arranged), Some(second.clone()));
    client
        .result("session.set_title", json!({ "id": id, "title": "Renamed" }))
        .await;
    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(second.clone())
    );

    drop(client);
    let (_core, mut client) = restarted(core).await;
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(second)
    );
}

/// A section's name is taken as a session's title is: each run of spacing
/// is one space, and a name holding any other control character is refused
/// with `invalid_params`, renaming included. Case is compared by Unicode
/// lowercase and nothing else, so two spellings of one accented letter are
/// two names.
// verifies: PROTO-CORE-API-R60
#[tokio::test(flavor = "multi_thread")]
async fn section_names_are_one_line_without_control_characters() {
    let (_core, mut client) = start().await;

    let spaced = create_section(&mut client, " Two \n\t  words  ").await;
    assert_eq!(spaced["name"], "Two words");
    // Taken once collapsed, ignoring case.
    let response = client
        .call("section.create", json!({ "name": "two   WORDS" }))
        .await;
    assert_eq!(response["error"]["code"], -32602, "{response:#}");

    for name in ["Bell\u{7}", "Null\u{0}here", "Esc\u{1b}[1m", "Del\u{7f}"] {
        let response = client.call("section.create", json!({ "name": name })).await;
        assert_valid("section.create", &response);
        assert_eq!(response["error"]["code"], -32602, "{name:?}: {response:#}");
        let response = client
            .call(
                "section.rename",
                json!({ "id": spaced["id"], "name": name }),
            )
            .await;
        assert_valid("section.rename", &response);
        assert_eq!(response["error"]["code"], -32602, "{name:?}: {response:#}");
    }

    let renamed = client
        .result(
            "section.rename",
            json!({ "id": spaced["id"], "name": "Three\r\n  words" }),
        )
        .await;
    assert_eq!(renamed["name"], "Three words");

    // "é" as one code point and as "e" with a combining accent: no
    // normalization, so both are made.
    create_section(&mut client, "Caf\u{e9}").await;
    create_section(&mut client, "Cafe\u{301}").await;
    assert_eq!(
        section_names(&mut client).await,
        vec![
            "Three words".to_string(),
            "Caf\u{e9}".to_string(),
            "Cafe\u{301}".to_string()
        ]
    );
}

/// Reads notifications until a `session.tile` for `id` that carries a
/// `last_activity_at`, failing after 10 seconds; returns its params and the
/// `ts` of the last `turn.completed` read on the way, if any.
async fn tile_with_activity(client: &mut Client, id: &str) -> (Value, Option<Value>) {
    let mut ended = None;
    let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let notification = client.notification().await.expect("connection open");
            let params = &notification["params"];
            match notification["method"].as_str() {
                Some("session.event") if params["event"]["event"]["type"] == "turn.completed" => {
                    ended = Some(params["event"]["ts"].clone());
                }
                Some("session.tile")
                    if params["session_id"] == id && params.get("last_activity_at").is_some() =>
                {
                    return params.clone();
                }
                _ => {}
            }
        }
    })
    .await
    .expect("a tile carrying the last activity within 10 s");
    (found, ended)
}

/// Every `session.tile` carries the session's `last_activity_at` as
/// `session.list` has it, and a turn's end sends a tile even when the tile
/// itself is unchanged, as the fake's `finish` turn leaves it.
// verifies: PROTO-CORE-API-R58
// verifies: PROTO-CORE-API-R59
#[tokio::test(flavor = "multi_thread")]
async fn a_tile_carries_the_last_activity_and_a_turns_end_sends_one() {
    let (core, mut client) = start().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    let before = session_row(&mut client, &id).await["tile"].clone();

    // A turn that replies and ends, changing nothing on the tile.
    client
        .result("session.send", json!({ "id": id, "text": "finish" }))
        .await;
    let (tile, ended) = tile_with_activity(&mut client, &id).await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["tile"], before, "the tile itself is unchanged");
    assert_eq!(tile["tile"], before);
    assert_eq!(tile["last_activity_at"], row["last_activity_at"]);
    if let Some(ended) = ended {
        assert_eq!(tile["last_activity_at"], ended);
    }

    // A tile sent for anything else carries the same value.
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
    let (tile, _) = tile_with_activity(&mut client, &id).await;
    assert_eq!(tile["tile"]["state"], "idle");
    assert_eq!(tile["last_activity_at"], row["last_activity_at"]);
}

/// The tile Core sends when a subscribe opens a stored session, after a
/// restart, carries the `last_activity_at` that `session.list` reports.
// verifies: PROTO-CORE-API-R59
#[tokio::test(flavor = "multi_thread")]
async fn the_tile_sent_on_opening_a_stored_session_carries_its_last_activity() {
    let records = fake::Records::default();
    let adapters: Vec<Arc<dyn Adapter>> =
        vec![Arc::new(FakeAdapter::with_records(Arc::clone(&records)))];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client
        .result("session.send", json!({ "id": id, "text": "finish" }))
        .await;
    tile_with_activity(&mut client, &id).await;
    client.result("session.stop", json!({ "id": id })).await;
    client.result("core.shutdown", json!({})).await;
    drop(client);
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();

    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(records))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;
    let listed = session_row(&mut client, &id).await["last_activity_at"].clone();
    assert!(listed.is_string(), "kept across the restart");
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let (tile, _) = tile_with_activity(&mut client, &id).await;
    assert_eq!(tile["last_activity_at"], listed);
}
