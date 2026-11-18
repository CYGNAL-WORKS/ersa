//! The user's arrangement of sessions (ADR 0026; PROTO-CORE-API, Sessions
//! and Sections; FLOW-ORGANIZE-SESSIONS), driven over the socket with the
//! fake backend: pins and sections set by `session.arrange`, the sections
//! themselves, each kept across a restart of Core, and `last_activity_at`,
//! which comes from the record and the turns.

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
// verifies: PROTO-CORE-API-R64
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

/// `unseen` is set and cleared by `session.arrange` alone, leaves the pin
/// and section as they were, is left by an arrangement that does not give
/// it, and outlives Core.
// verifies: PROTO-CORE-API-R78
#[tokio::test(flavor = "multi_thread")]
async fn unseen_is_set_only_when_given_and_outlives_core() {
    let (core, mut client) = start().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    let other = new_session(&mut client, &workspace_id).await;
    let work = create_section(&mut client, "Work").await;

    // A session listed for the first time is not unseen.
    assert_eq!(session_row(&mut client, &id).await["unseen"], false);

    arrange(
        &mut client,
        json!({ "id": id, "pinned": true, "section_id": work["id"] }),
    )
    .await;
    // Marked unseen; the pin and section are left as they were.
    let arranged = arrange(&mut client, json!({ "id": id, "unseen": true })).await;
    assert_eq!(arranged["unseen"], true);
    assert_eq!(arranged["pinned"], true);
    assert_eq!(arranged["section_id"], work["id"]);
    assert_eq!(session_row(&mut client, &id).await["unseen"], true);
    // Only that session.
    assert_eq!(session_row(&mut client, &other).await["unseen"], false);

    // Pin and section alone leave the mark.
    let arranged = arrange(
        &mut client,
        json!({ "id": id, "pinned": false, "section_id": null }),
    )
    .await;
    assert_eq!(arranged["unseen"], true);

    drop(client);
    let (core, mut client) = restarted(core).await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["unseen"], true, "kept across the restart");
    assert_eq!(row["pinned"], false);
    assert_eq!(session_row(&mut client, &other).await["unseen"], false);

    // Cleared, and that outlives Core too.
    let arranged = arrange(&mut client, json!({ "id": id, "unseen": false })).await;
    assert_eq!(arranged["unseen"], false);
    arrange(&mut client, json!({ "id": other, "unseen": true })).await;
    drop(client);
    let (_core, mut client) = restarted(core).await;
    assert_eq!(session_row(&mut client, &id).await["unseen"], false);
    assert_eq!(session_row(&mut client, &other).await["unseen"], true);
}

/// A section that does not exist is `not_found`, and the session is left
/// as it was, the pin given beside it included.
// verifies: PROTO-CORE-API-R64
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

/// Section names follow Claude Desktop's rules: Core trims a name and
/// refuses, with `invalid_params`, one that is then blank or longer than
/// 200 characters; `section.create` with exactly another section's name
/// answers with that section and makes none; `section.rename` to exactly
/// another's name is refused; and names that differ only in case are
/// different sections. A refusal changes nothing, and all of it outlives Core.
// verifies: PROTO-CORE-API-R76
#[tokio::test(flavor = "multi_thread")]
async fn section_names_follow_claude_desktops_rules() {
    let (core, mut client) = start().await;

    let work = create_section(&mut client, "  Work \t").await;
    assert_eq!(work["name"], "Work");
    assert!(work["id"].as_str().is_some_and(|id| !id.is_empty()));
    // 200 characters, not bytes: each of these is two bytes in UTF-8.
    let longest = "é".repeat(200);
    let summer = create_section(&mut client, &longest).await;
    assert_eq!(summer["name"], longest.as_str());

    // Exactly another's name, once trimmed: that section, the same one in
    // every field, and no second section.
    for name in ["Work", " Work  "] {
        assert_eq!(create_section(&mut client, name).await, work, "{name:?}");
    }
    assert_eq!(sections(&mut client).await, json!([work, summer]));

    // Another case is another name, beyond ASCII too.
    let lower = create_section(&mut client, "work").await;
    let upper = create_section(&mut client, "É".repeat(200).as_str()).await;
    assert_ne!(lower["id"], work["id"]);
    assert_eq!(lower["name"], "work");
    assert_ne!(upper["id"], summer["id"]);
    let all = json!([work, summer, lower, upper]);
    assert_eq!(sections(&mut client).await, all);

    let refused_names = [
        String::new(),
        " \t ".to_string(),
        "é".repeat(201),
        "x".repeat(201),
    ];
    for name in &refused_names {
        let response = client.call("section.create", json!({ "name": name })).await;
        assert_valid("section.create", &response);
        assert_eq!(response["error"]["code"], -32602, "{name:?}: {response:#}");
    }
    assert_eq!(sections(&mut client).await, all);

    // Renaming: the same limits, and exactly another's name is refused.
    let taken = [" work ".to_string(), longest.clone()];
    for name in refused_names.iter().chain(&taken) {
        let response = client
            .call("section.rename", json!({ "id": work["id"], "name": name }))
            .await;
        assert_valid("section.rename", &response);
        assert_eq!(response["error"]["code"], -32602, "{name:?}: {response:#}");
    }
    assert_eq!(sections(&mut client).await, all);

    // A section may take its own name again, or another case of another's.
    let response = client
        .call(
            "section.rename",
            json!({ "id": work["id"], "name": "Work" }),
        )
        .await;
    assert_valid("section.rename", &response);
    assert_eq!(response["result"], work);
    let renamed = client
        .result(
            "section.rename",
            json!({ "id": work["id"], "name": " WORK " }),
        )
        .await;
    assert_eq!(renamed["name"], "WORK");
    assert_eq!(renamed["id"], work["id"]);
    assert_eq!(renamed["created_at"], work["created_at"]);
    // Its old name is free once it is renamed.
    let again = create_section(&mut client, "Work").await;
    assert_ne!(again["id"], work["id"]);

    let error = client
        .error(
            "section.rename",
            json!({ "id": "sec-missing", "name": "x" }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");

    // The rules hold after a restart: the names Core stored are compared
    // exactly.
    drop(client);
    let (_core, mut client) = restarted(core).await;
    assert_eq!(create_section(&mut client, "work").await, lower);
    assert_eq!(create_section(&mut client, "WORK").await, renamed);
    assert_eq!(
        section_names(&mut client).await,
        vec![
            "WORK".to_string(),
            longest,
            "work".to_string(),
            "É".repeat(200),
            "Work".to_string()
        ]
    );
}

/// `section.remove` puts the sessions it held in no section, keeping every
/// session, and that outlives Core.
// verifies: PROTO-CORE-API-R39
#[tokio::test(flavor = "multi_thread")]
async fn a_section_made_before_core_restarted_takes_a_session_and_is_removed() {
    let (core, mut client) = start().await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    let work = create_section(&mut client, "Work").await;
    drop(client);

    // The new Core knows the section from its store.
    let (_core, mut client) = restarted(core).await;
    arrange(&mut client, json!({ "id": id, "section_id": work["id"] })).await;
    let row = session_row(&mut client, &id).await;
    assert_eq!(row["section_id"], work["id"]);
    let removed = client
        .result("section.remove", json!({ "id": work["id"] }))
        .await;
    assert_eq!(removed, work);
    let row = session_row(&mut client, &id).await;
    assert_eq!(row.get("section_id"), None);
}

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

/// Core over the fake backend sharing `records`, on data directory `dir`.
async fn start_on(dir: tempfile::TempDir, records: &fake::Records) -> (TestCore, Client) {
    let adapters: Vec<Arc<dyn Adapter>> =
        vec![Arc::new(FakeAdapter::with_records(Arc::clone(records)))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let client = core.client().await;
    (core, client)
}

/// Shuts `core` down and hands back its data directory.
async fn shut_down(core: TestCore) -> tempfile::TempDir {
    let mut client = core.client().await;
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    dir
}

/// Lists sessions until session `id`'s `last_activity_at` is `want`,
/// failing after 10 seconds: a session's record is read again once its run
/// has ended, which may land after the end is published.
async fn until_last_activity(client: &mut Client, id: &str, want: Option<&str>) -> Value {
    let want = want.map(|at| json!(at));
    let found = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let row = session_row(client, id).await;
            if row.get("last_activity_at").cloned() == want {
                return row;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("last activity {want:?} within 10 s"))
}

/// The time the fake's record gives the reply to a message that is not a
/// command (`fake::recorded_reply`).
const RECORDED_REPLY_AT: &str = "2026-11-01T00:00:01.000Z";

/// A session's last activity, live, is the `ts` of its latest
/// `turn.completed` since it went live, whatever its outcome, else what its
/// record gave when it went live; not live, its record's latest
/// conversation entry, absent when there is none. Core stores none, so no
/// stored time outlives the record.
// verifies: PROTO-CORE-API-R75
#[tokio::test(flavor = "multi_thread")]
async fn last_activity_comes_from_the_turns_while_live_and_from_the_record_otherwise() {
    let records = fake::Records::default();
    let (core, mut client) = start_on(tempdir(), &records).await;
    let workspace_id = json!(workspace(&mut client, core.dir.path()).await);
    let id = new_session(&mut client, &workspace_id).await;
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    let last_activity = |row: &Value| row.get("last_activity_at").cloned();
    assert_eq!(last_activity(&session_row(&mut client, &id).await), None);

    // Live, the record gains a turn's entries, but no turn has ended since
    // the session went live, and its record gave none then.
    client
        .result("session.send", json!({ "id": id, "text": "hello" }))
        .await;
    client.events_until("assistant.text").await;
    let response = client.call("session.list", json!({})).await;
    assert_valid("session.list", &response);
    assert_eq!(last_activity(&session_row(&mut client, &id).await), None);

    // A turn that keeps working sets none; its end, interrupted, does.
    client
        .result("session.send", json!({ "id": id, "text": "work" }))
        .await;
    client.events_until("turn.started").await;
    assert_eq!(last_activity(&session_row(&mut client, &id).await), None);
    client
        .result("session.interrupt", json!({ "id": id }))
        .await;
    let ended = client.events_until("turn.completed").await;
    let interrupted = ended.last().unwrap()["ts"].clone();
    assert!(interrupted.is_string());
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(interrupted)
    );
    // A turn that ends successfully: its time.
    client
        .result("session.send", json!({ "id": id, "text": "finish" }))
        .await;
    let ended = client.events_until("turn.completed").await;
    let finished = ended.last().unwrap()["ts"].clone();
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(finished.clone())
    );
    assert_ne!(finished, json!(RECORDED_REPLY_AT));

    // Stopped, it is not live: its record's latest conversation entry.
    client.result("session.stop", json!({ "id": id })).await;
    client.events_until("session.ended").await;
    until_last_activity(&mut client, &id, Some(RECORDED_REPLY_AT)).await;

    // Work done elsewhere shows the next time sessions are listed.
    let backend_id = records.lock().unwrap().keys().next().cloned().unwrap();
    let mut later = fake::recorded_user("u-elsewhere", "from the terminal");
    later.ts = Some("2026-11-05T00:00:00.000Z".into());
    records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(later);
    let row = session_row(&mut client, &id).await;
    assert_eq!(last_activity(&row), Some(json!("2026-11-05T00:00:00.000Z")));
    drop(client);

    // Nothing stored outlives the record: after a restart, a record with no
    // conversation left gives none.
    let dir = shut_down(core).await;
    records.lock().unwrap().insert(
        backend_id.clone(),
        vec![fake::recorded_title("Only a title")],
    );
    let (core, mut client) = start_on(dir, &records).await;
    assert_eq!(last_activity(&session_row(&mut client, &id).await), None);
    drop(client);

    // Gone live again, it shows what its record gave until a turn ends.
    let dir = shut_down(core).await;
    let mut entry = fake::recorded_reply("r-old", "old");
    entry.ts = Some("2026-11-07T00:00:00.000Z".into());
    records
        .lock()
        .unwrap()
        .get_mut(&backend_id)
        .unwrap()
        .push(entry);
    let (_core, mut client) = start_on(dir, &records).await;
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(json!("2026-11-07T00:00:00.000Z"))
    );
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.result("session.resume", json!({ "id": id })).await;
    client.events_until("session.started").await;
    client
        .result("session.send", json!({ "id": id, "text": "hello again" }))
        .await;
    client.events_until("assistant.text").await;
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(json!("2026-11-07T00:00:00.000Z"))
    );
    client
        .result("session.send", json!({ "id": id, "text": "finish" }))
        .await;
    let ended = client.events_until("turn.completed").await;
    assert_eq!(
        last_activity(&session_row(&mut client, &id).await),
        Some(ended.last().unwrap()["ts"].clone())
    );
}

/// A section's name is taken as a session's title is: each run of spacing
/// is one space, and a name holding any other control character is refused
/// with `invalid_params`, renaming included. Names are compared character
/// for character, without normalizing, so two spellings of one accented
/// letter are two names.
// verifies: PROTO-CORE-API-R60
#[tokio::test(flavor = "multi_thread")]
async fn section_names_are_one_line_without_control_characters() {
    let (_core, mut client) = start().await;

    let spaced = create_section(&mut client, " Two \n\t  words  ").await;
    assert_eq!(spaced["name"], "Two words");
    // Exactly that name once collapsed: the same section.
    assert_eq!(create_section(&mut client, "Two 	 words").await, spaced);

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
    assert_eq!(listed, json!(RECORDED_REPLY_AT), "from the record");
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    let (tile, _) = tile_with_activity(&mut client, &id).await;
    assert_eq!(tile["last_activity_at"], listed);
}
