//! The Core API driven over the socket: handshake, errors, reserved methods,
//! core, hosts, backends, settings, and shutdown. Projects and workspaces are
//! in `workspaces.rs`.

mod support;

use std::sync::Arc;

use ersa_adapter::Adapter;
use ersa_core::{ServeError, Server};
use ersa_protocol::core_api::ModelChoice;
use serde_json::json;
use support::fake::{FakeAdapter, ModelsAnswer};
use support::suite::{self, Target};
use support::{assert_valid, kind, start, start_with_adapters, tempdir, test_config};

// The shared request/response suite, run on Core; `replay.rs` runs it on
// the replay server.

// verifies: PROTO-CORE-API-R01
#[tokio::test(flavor = "multi_thread")]
async fn every_method_before_hello_is_refused_with_invalid_state() {
    suite::every_method_before_hello_is_refused_with_invalid_state(Target::Core).await;
}

// verifies: PROTO-CORE-API-R01
#[tokio::test(flavor = "multi_thread")]
async fn hello_reports_versions_and_the_local_host() {
    suite::hello_reports_versions_and_the_local_host(Target::Core).await;
}

// verifies: PROTO-CORE-API-R07
#[tokio::test(flavor = "multi_thread")]
async fn hello_without_a_shared_version_names_the_supported_ones_and_closes() {
    suite::hello_without_a_shared_version_names_the_supported_ones_and_closes(Target::Core).await;
}

// verifies: PROTO-CORE-API-R10
#[tokio::test(flavor = "multi_thread")]
async fn reserved_methods_answer_invalid_state_naming_their_milestone() {
    suite::reserved_methods_answer_invalid_state_naming_their_milestone(Target::Core).await;
}

/// `session.resume` is built: an unknown session is `not_found`, as for
/// any other method naming one.
#[tokio::test(flavor = "multi_thread")]
async fn session_resume_of_an_unknown_session_is_not_found() {
    let core = start().await;
    let mut client = core.client().await;
    let error = client.error("session.resume", json!({ "id": "s-1" })).await;
    assert_eq!(kind(&error), "not_found");
}

// verifies: PROTO-CORE-API-R06
#[tokio::test(flavor = "multi_thread")]
async fn protocol_errors_use_the_standard_codes() {
    suite::protocol_errors_use_the_standard_codes(Target::Core).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_is_not_found() {
    suite::an_unknown_session_is_not_found(Target::Core).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn client_notifications_get_no_answer() {
    suite::client_notifications_get_no_answer(Target::Core).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pipelined_requests_are_answered_in_order() {
    suite::pipelined_requests_are_answered_in_order(Target::Core).await;
}

// verifies: PROTO-CORE-API-R06
#[tokio::test(flavor = "multi_thread")]
async fn status_reports_the_database_and_no_sessions() {
    let core = start().await;
    let mut client = core.client().await;
    // Params may be omitted for a method without any.
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "core.status" });
    client.send_line(&request.to_string()).await;
    let response = client.read_line().await.expect("a response");
    assert_valid("core.status", &response);
    let status = &response["result"];
    assert_eq!(status["sessions_running"], json!(0));
    assert_eq!(status["sessions_waiting"], json!(0));
    let db_path = status["db_path"].as_str().unwrap();
    assert!(
        std::path::Path::new(db_path).starts_with(core.data_dir()),
        "{db_path}"
    );
}

// verifies: PROTO-CORE-API-R06
// verifies: PROTO-CORE-API-R65
#[tokio::test(flavor = "multi_thread")]
async fn host_and_backend_lists_hold_each_registered_adapter_in_order() {
    // Core names no backend of its own: with no adapter registered, the
    // host has none.
    let core = start().await;
    let mut client = core.client().await;
    let hosts = client.result("host.list", json!({})).await;
    assert_eq!(hosts[0]["backends"], json!([]));
    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    assert_eq!(infos, json!([]));

    // Registered adapters are listed once each, in the order registered,
    // whatever their names. A second registration of a name is dropped.
    let adapters: Vec<Arc<dyn Adapter>> = vec![
        Arc::new(FakeAdapter::named("zeta")),
        Arc::new(FakeAdapter::named("alpha")),
        Arc::new(FakeAdapter::named("zeta")),
    ];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;

    let hosts = client.result("host.list", json!({})).await;
    let hosts = hosts.as_array().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0]["id"], json!("local"));
    assert_eq!(hosts[0]["kind"], json!("local"));
    assert_eq!(hosts[0]["status"], json!("connected"));
    let names = |list: &serde_json::Value| -> Vec<String> {
        list.as_array()
            .unwrap()
            .iter()
            .map(|b| b["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(names(&hosts[0]["backends"]), ["zeta", "alpha"]);
    // The test Core points detection at a program that does not exist.
    assert!(
        hosts[0]["backends"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["status"] == json!("missing"))
    );

    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    assert_eq!(names(&infos), ["zeta", "alpha"]);
    // A missing backend lists no models (PROTO-CORE-API-R72).
    assert_eq!(infos[0]["models"], json!([]));

    let error = client
        .error("backend.list", json!({ "host_id": "nowhere" }))
        .await;
    assert_eq!(kind(&error), "not_found");
}

/// `backend.list` on a Core that detects the fake backend for real, with
/// the fake answering `models` as given.
async fn listed(answer: ModelsAnswer) -> serde_json::Value {
    let fake = FakeAdapter::new(1);
    *fake.models.lock().unwrap() = answer;
    let core = start_with_adapters(tempdir(), support::detecting, vec![Arc::new(fake)]).await;
    let mut client = core.client().await;
    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    infos[0].clone()
}

/// A backend's models are its own report, every field as it gave it; one
/// that cannot report them is `error`, and one the probe cannot find
/// `missing`, either with no models.
// verifies: PROTO-CORE-API-R72
#[tokio::test(flavor = "multi_thread")]
async fn backend_list_lists_the_models_the_backend_reports() {
    let listed_models = |models: Vec<ModelChoice>| listed(ModelsAnswer::Listed(models));
    let mut reported = support::fake::fake_models();
    reported[0].label = "Default (recommended)".into();
    reported[0].resolved = Some("model-a-7".into());
    let info = listed_models(reported).await;
    assert_eq!(info["status"], json!("available"));
    assert_eq!(info["version"], json!("1.0.0"));
    assert_eq!(
        info["models"],
        json!([
            { "id": "model-a", "label": "Default (recommended)", "default": true, "resolved": "model-a-7" },
            { "id": "model-b", "label": "MODEL-B", "default": false, "resolved": "model-b-1", "efforts": ["low"] },
        ])
    );

    let info = listed(ModelsAnswer::Fails).await;
    assert_eq!(info["status"], json!("error"));
    assert_eq!(info["models"], json!([]));

    let info = listed(ModelsAnswer::Missing).await;
    assert_eq!(info["status"], json!("missing"));
    assert_eq!(info["models"], json!([]));
}

// verifies: PROTO-CORE-API-R06
#[tokio::test(flavor = "multi_thread")]
async fn settings_are_patch_merged_and_persisted() {
    let core = start().await;
    let mut client = core.client().await;
    assert_eq!(client.result("settings.get", json!({})).await, json!({}));

    let root = core.dir.path().join("worktrees").display().to_string();
    let after = client
        .result(
            "settings.set",
            json!({ "patch": { "workspaces": { "root": root }, "theme": "dark" } }),
        )
        .await;
    assert_eq!(
        after,
        json!({ "workspaces": { "root": root }, "theme": "dark" })
    );

    let after = client
        .result(
            "settings.set",
            json!({ "patch": { "theme": null, "workspaces": { "post_create": "npm ci" } } }),
        )
        .await;
    let expected = json!({ "workspaces": { "root": root, "post_create": "npm ci" } });
    assert_eq!(after, expected);

    // A second connection sees the same settings.
    let mut other = core.client().await;
    assert_eq!(other.result("settings.get", json!({})).await, expected);

    // A wrongly typed known setting is refused and nothing changes.
    let error = client
        .error(
            "settings.set",
            json!({ "patch": { "workspaces": { "root": 42 } } }),
        )
        .await;
    assert_eq!(error["code"], json!(-32602));
    assert_eq!(client.result("settings.get", json!({})).await, expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_answers_then_stops_core() {
    let core = start().await;
    let mut client = core.client().await;
    let mut bystander = core.client().await;

    assert_eq!(client.result("core.shutdown", json!({})).await, json!({}));
    tokio::time::timeout(std::time::Duration::from_secs(10), core.task)
        .await
        .expect("Core stopped in time")
        .expect("the server task did not panic")
        .expect("the server stopped cleanly");
    assert_eq!(
        bystander.read_line().await,
        None,
        "other clients are closed"
    );

    // The endpoint is free again.
    let restarted = Server::bind(test_config(&core.dir.path().join("data"), &core.endpoint)).await;
    assert!(restarted.is_ok(), "rebinding after shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_core_on_the_same_endpoint_refuses_to_start() {
    let core = start().await;
    let other_dir = support::tempdir();
    let second = Server::bind(test_config(other_dir.path(), &core.endpoint)).await;
    match second {
        Err(ServeError::AlreadyRunning { endpoint }) => assert_eq!(endpoint, core.endpoint),
        Err(other) => panic!("expected AlreadyRunning, got {other}"),
        Ok(_) => panic!("a second Core bound the same endpoint"),
    }
    // The first Core still serves.
    core.client().await;
}

#[test]
fn an_endpoint_label_cannot_leave_its_name() {
    // The label lands in a pipe or file name: no separators, no `..`.
    for label in ["", "a/b", r"a\b", "..", "a b", &"x".repeat(65)] {
        let result = ersa_core::platform::labelled_endpoint(label);
        assert!(result.is_err(), "{label:?} gave {result:?}");
    }
    assert!(ersa_core::platform::labelled_endpoint("shell-test_42").is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn several_clients_are_served_at_once() {
    let core = start().await;
    let mut clients = Vec::new();
    for _ in 0..4 {
        clients.push(core.client().await);
    }
    for client in &mut clients {
        client.result("core.status", json!({})).await;
    }
}
