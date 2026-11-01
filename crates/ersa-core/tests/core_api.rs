//! The Core API driven over the socket: handshake, errors, reserved methods,
//! core, hosts, backends, settings, and shutdown. Projects and workspaces are
//! in `workspaces.rs`.

mod support;

use ersa_core::{ServeError, Server};
use ersa_protocol::core_api::Method;
use ersa_protocol::events::PROJECTION_VERSION;
use serde_json::{Value, json};
use support::{Client, assert_valid, kind, start, test_config};

// verifies: PROTO-CORE-API-R01
#[tokio::test(flavor = "multi_thread")]
async fn every_method_before_hello_is_refused_with_invalid_state() {
    let core = start().await;
    let mut client = Client::connect(&core.endpoint).await;

    // A real method, an unknown one, and a reserved one: all refused alike.
    for method in ["project.list", "core.status", "no.such_method", "host.add"] {
        let error = client.error(method, json!({})).await;
        assert_eq!(kind(&error), "invalid_state", "{method}: {error}");
        assert!(error["message"].as_str().unwrap().contains("core.hello"));
    }

    // The connection stays usable, and hello then unlocks everything.
    client.hello().await;
    client.result("project.list", json!({})).await;
}

// verifies: PROTO-CORE-API-R01
#[tokio::test(flavor = "multi_thread")]
async fn hello_reports_versions_and_the_local_host() {
    let core = start().await;
    let mut client = Client::connect(&core.endpoint).await;
    let hello = client
        .result(
            "core.hello",
            json!({ "client": "test", "client_version": "0", "api_versions": [1, 7] }),
        )
        .await;
    assert_eq!(hello["api_version"], json!(1), "the highest shared version");
    assert_eq!(hello["event_projection_version"], json!(PROJECTION_VERSION));
    assert_eq!(hello["host"]["id"], json!("local"));
    assert_eq!(hello["host"]["platform"], json!(std::env::consts::OS));
    assert_eq!(hello["core_version"], json!(env!("CARGO_PKG_VERSION")));

    // A second hello on the same connection is a mistake.
    let error = client
        .error(
            "core.hello",
            json!({ "client": "test", "client_version": "0", "api_versions": [1] }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state");
}

// verifies: PROTO-CORE-API-R07
#[tokio::test(flavor = "multi_thread")]
async fn hello_without_a_shared_version_names_the_supported_ones_and_closes() {
    let core = start().await;
    let mut client = Client::connect(&core.endpoint).await;
    let error = client
        .error(
            "core.hello",
            json!({ "client": "test", "client_version": "0", "api_versions": [2, 3] }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("supports [1]"), "{message}");
    assert!(message.contains("[2, 3]"), "{message}");

    assert_eq!(client.read_line().await, None, "Core closed the connection");

    // Other clients are unaffected.
    core.client().await;
}

// verifies: PROTO-CORE-API-R10
#[tokio::test(flavor = "multi_thread")]
async fn reserved_methods_answer_invalid_state_naming_their_milestone() {
    let core = start().await;
    let mut client = core.client().await;
    let reserved: Vec<_> = Method::ALL
        .iter()
        .filter_map(|m| m.reserved_for().map(|milestone| (m.name(), milestone)))
        .collect();
    assert!(!reserved.is_empty());
    for (method, milestone) in reserved {
        // Params are not checked: the answer does not depend on them.
        let error = client.error(method, json!({})).await;
        assert_eq!(
            error["code"],
            json!(-32000),
            "{method}: an application error"
        );
        assert_eq!(kind(&error), "invalid_state", "{method}");
        let message = error["message"].as_str().unwrap();
        assert!(message.contains(milestone), "{method}: {message}");
    }
}

// verifies: PROTO-CORE-API-R10
#[tokio::test(flavor = "multi_thread")]
async fn session_methods_answer_invalid_state_until_m0_t6() {
    let core = start().await;
    let mut client = core.client().await;
    for method in Method::ALL
        .iter()
        .filter(|m| m.name().starts_with("session."))
    {
        let error = client.error(method.name(), json!({ "id": "s-1" })).await;
        assert_eq!(kind(&error), "invalid_state", "{}", method.name());
        assert!(error["message"].as_str().unwrap().contains("M0-T6"));
    }
}

// verifies: PROTO-CORE-API-R06
#[tokio::test(flavor = "multi_thread")]
async fn protocol_errors_use_the_standard_codes() {
    let core = start().await;
    let mut client = core.client().await;

    client.send_line("{ this is not json").await;
    let response = client.read_line().await.expect("a response");
    assert_valid("core.status", &response);
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], json!(-32700));

    client.send_line(r#"{"jsonrpc":"2.0","id":"x"}"#).await;
    let response = client.read_line().await.expect("a response");
    assert_valid("core.status", &response);
    assert_eq!(response["id"], json!("x"));
    assert_eq!(response["error"]["code"], json!(-32600));

    let error = client.error("no.such_method", json!({})).await;
    assert_eq!(error["code"], json!(-32601));

    let error = client.error("project.add", json!({ "path": 3 })).await;
    assert_eq!(error["code"], json!(-32602));

    let error = client
        .error("core.status", json!({ "unexpected": true }))
        .await;
    assert_eq!(error["code"], json!(-32602));
}

#[tokio::test(flavor = "multi_thread")]
async fn client_notifications_get_no_answer() {
    let core = start().await;
    let mut client = core.client().await;
    client
        .send_line(r#"{"jsonrpc":"2.0","method":"interface.focused","params":{}}"#)
        .await;
    // The next line Core writes answers the next request, not the notification.
    client.result("project.list", json!({})).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pipelined_requests_are_answered_in_order() {
    let core = start().await;
    let mut client = core.client().await;
    let lines: Vec<String> = (100..110)
        .map(|id| json!({ "jsonrpc": "2.0", "id": id, "method": "core.status" }).to_string())
        .collect();
    client.send_line(&lines.join("\n")).await;
    for id in 100..110 {
        let response = client.read_line().await.expect("a response");
        assert_valid("core.status", &response);
        assert_eq!(response["id"], json!(id));
    }
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
#[tokio::test(flavor = "multi_thread")]
async fn host_and_backend_lists_describe_the_local_host() {
    let core = start().await;
    let mut client = core.client().await;

    let hosts = client.result("host.list", json!({})).await;
    let hosts = hosts.as_array().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(hosts[0]["id"], json!("local"));
    assert_eq!(hosts[0]["kind"], json!("local"));
    assert_eq!(hosts[0]["status"], json!("connected"));
    let backends = hosts[0]["backends"].as_array().unwrap();
    assert_eq!(backends[0]["name"], json!("claude-code"));
    // The test Core points detection at a program that does not exist.
    assert_eq!(backends[0]["status"], json!("missing"));

    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    assert_eq!(infos[0]["name"], json!("claude-code"));
    assert!(infos[0]["min_tested_version"].is_string());
    assert!(infos[0]["max_tested_version"].is_string());

    let error = client
        .error("backend.list", json!({ "host_id": "nowhere" }))
        .await;
    assert_eq!(kind(&error), "not_found");
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
    let other_dir = tempfile::tempdir().expect("temp dir");
    let second = Server::bind(test_config(other_dir.path(), &core.endpoint)).await;
    match second {
        Err(ServeError::AlreadyRunning { endpoint }) => assert_eq!(endpoint, core.endpoint),
        Err(other) => panic!("expected AlreadyRunning, got {other}"),
        Ok(_) => panic!("a second Core bound the same endpoint"),
    }
    // The first Core still serves.
    core.client().await;
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
