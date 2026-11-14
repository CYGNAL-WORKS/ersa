//! The replay server: Core serving a recorded session over the real socket,
//! with no backend behind it (ARCH-CORE, Implementation decisions;
//! PROTO-CORE-API, Conformance). The recording is the fake backend's; the
//! comparison of a replay with a live turn of a real backend is in that
//! backend's adapter's tests.

mod support;

use std::time::Duration;

use ersa_adapter::Direction;
use ersa_core::replay::Pace;
use serde_json::{Value, json};
use support::fake::{BACKEND, recording, start_replay_with};
use support::suite::{self, Target};
use support::{Client, TestCore, kind, the_session};

async fn start_replay(pace: Pace) -> TestCore {
    start_replay_with(|_| {}, pace).await
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
    // The recording's project has no version control, so Core refuses it as
    // it does any such project: the method runs, read-only, as on a live Core.
    let error = client
        .error("branch.list", json!({ "project_id": projects[0]["id"] }))
        .await;
    assert_eq!(error["data"]["kind"], "invalid_state");
    // Its one workspace is the project's own folder, which Core refuses to
    // count, since nothing of it is ever deleted.
    let error = client
        .error(
            "workspace.changes",
            json!({ "id": session["workspace_id"] }),
        )
        .await;
    assert_eq!(error["data"]["kind"], "invalid_state");
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
            json!({ "workspace_id": workspace_id, "backend": BACKEND }),
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
        (
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        ),
        ("session.set_mode", json!({ "id": id, "mode": "m" })),
        ("session.arrange", json!({ "id": id, "pinned": true })),
        ("section.create", json!({ "name": "Work" })),
        ("section.rename", json!({ "id": "sec-1", "name": "Work" })),
        ("section.remove", json!({ "id": "sec-1" })),
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
    assert_eq!(after["archived"], false);
    assert_eq!(after["pinned"], false);
    // Reading the sections changes nothing, so the replay answers it.
    assert_eq!(client.result("section.list", json!({})).await, json!([]));
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
