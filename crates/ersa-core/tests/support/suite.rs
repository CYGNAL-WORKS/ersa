//! The request/response suite every server speaking the Core API passes:
//! Core, and the replay server with a recording in place of a backend
//! (PROTO-CORE-API, Conformance). Each check takes the [`Target`] to run
//! against; `core_api.rs` and `connection_limits.rs` run them on Core,
//! `replay.rs` on the replay server.
//!
//! Only what holds for both lives here: what depends on Core's contents
//! (an empty session list, settings that change) stays with Core's tests.

use std::time::Duration;

use ersa_core::CoreConfig;
use ersa_protocol::core_api::Method;
use ersa_protocol::events::PROJECTION_VERSION;
use serde_json::{Value, json};

use super::claude::start_replay_with;
use super::{Client, TestCore, assert_valid, kind, start_with, tempdir};

/// Which server a check runs against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Core,
    /// The replay server serving fixture 11, all at once.
    Replay,
}

impl Target {
    pub async fn start(self) -> TestCore {
        self.start_with(|_| {}).await
    }

    pub async fn start_with(self, configure: impl FnOnce(&mut CoreConfig)) -> TestCore {
        match self {
            Target::Core => start_with(tempdir(), configure).await,
            Target::Replay => start_replay_with(configure, ersa_core::replay::Pace::Instant).await,
        }
    }
}

pub async fn every_method_before_hello_is_refused_with_invalid_state(target: Target) {
    let core = target.start().await;
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

pub async fn hello_reports_versions_and_the_local_host(target: Target) {
    let core = target.start().await;
    let mut client = Client::connect(&core.endpoint).await;
    let hello = client
        .result(
            "core.hello",
            json!({ "client": "test", "client_version": "0", "api_versions": [3, 7] }),
        )
        .await;
    assert_eq!(hello["api_version"], json!(3), "the highest shared version");
    assert_eq!(hello["event_projection_version"], json!(PROJECTION_VERSION));
    assert_eq!(hello["host"]["id"], json!("local"));
    assert_eq!(hello["host"]["platform"], json!(std::env::consts::OS));
    assert_eq!(hello["core_version"], json!(env!("CARGO_PKG_VERSION")));

    // A second hello on the same connection is a mistake.
    let error = client
        .error(
            "core.hello",
            json!({ "client": "test", "client_version": "0", "api_versions": [3] }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state");
}

pub async fn hello_without_a_shared_version_names_the_supported_ones_and_closes(target: Target) {
    let core = target.start().await;
    let mut client = Client::connect(&core.endpoint).await;
    let error = client
        .error(
            "core.hello",
            json!({ "client": "test", "client_version": "0", "api_versions": [1, 2] }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("supports [3]"), "{message}");
    assert!(message.contains("[1, 2]"), "{message}");

    assert_eq!(client.read_line().await, None, "Core closed the connection");

    // Other clients are unaffected.
    core.client().await;
}

pub async fn reserved_methods_answer_invalid_state_naming_their_milestone(target: Target) {
    let core = target.start().await;
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

pub async fn protocol_errors_use_the_standard_codes(target: Target) {
    let core = target.start().await;
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

    let error = client
        .error("workspace.list", json!({ "project_id": 3 }))
        .await;
    assert_eq!(error["code"], json!(-32602));

    let error = client
        .error("core.status", json!({ "unexpected": true }))
        .await;
    assert_eq!(error["code"], json!(-32602));
}

pub async fn an_unknown_session_is_not_found(target: Target) {
    let core = target.start().await;
    let mut client = core.client().await;
    for method in ["session.raw_log", "session.subscribe"] {
        let params = match method {
            "session.subscribe" => json!({ "id": "s-nope", "from_seq": 0 }),
            _ => json!({ "id": "s-nope" }),
        };
        let error = client.error(method, params).await;
        assert_eq!(kind(&error), "not_found", "{method}: {error:#}");
    }
}

pub async fn client_notifications_get_no_answer(target: Target) {
    let core = target.start().await;
    let mut client = core.client().await;
    client
        .send_line(r#"{"jsonrpc":"2.0","method":"interface.focused","params":{}}"#)
        .await;
    // The next line Core writes answers the next request, not the notification.
    client.result("project.list", json!({})).await;
}

pub async fn pipelined_requests_are_answered_in_order(target: Target) {
    let core = target.start().await;
    let mut client = core.client().await;
    let lines: Vec<String> = (100..110)
        .map(|id| json!({ "jsonrpc": "2.0", "id": id, "method": "core.status" }).to_string())
        .collect();
    client.send_line(&lines.join("\n")).await;
    for id in 100..110 {
        // Tiles may arrive in between on a server with a session.
        let response = loop {
            let line = client.read_line().await.expect("a response");
            if line.get("id").is_some() {
                break line;
            }
        };
        assert_valid("core.status", &response);
        assert_eq!(response["id"], json!(id));
    }
}

/// Reads the parse error Core sends for an unreadable line, then checks the
/// connection was closed. Notifications sent before it are skipped.
async fn expect_parse_error_then_close(client: &mut Client) {
    let response = loop {
        let line = client.read_line().await.expect("a parse error first");
        if line.get("id").is_some() {
            break line;
        }
    };
    assert_valid("core.status", &response);
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], json!(-32700));
    loop {
        match client.read_line().await {
            None => break,
            Some(line) => assert!(line.get("id").is_none(), "then Core closes: {line:#}"),
        }
    }
}

pub async fn a_line_over_the_cap_gets_a_parse_error_and_closes(target: Target) {
    let core = target
        .start_with(|config| config.max_line_bytes = 1024)
        .await;

    // Exactly at the cap is fine (a valid request padded with spaces).
    let mut client = core.client().await;
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "core.status" }).to_string();
    let padded = format!("{request:<1024}");
    client.send_line(&padded).await;
    let response = loop {
        let line = client.read_line().await.expect("an answer");
        if line.get("id").is_some() {
            break line;
        }
    };
    assert_eq!(response["id"], json!(1));

    // One byte over is not, even with no newline yet in sight.
    let over = format!("{request:<1025}");
    client.send_bytes(over.as_bytes()).await;
    expect_parse_error_then_close(&mut client).await;

    // Before the handshake too.
    let mut fresh = Client::connect(&core.endpoint).await;
    fresh.send_bytes(&vec![b'x'; 4096]).await;
    expect_parse_error_then_close(&mut fresh).await;
}

pub async fn a_line_that_is_not_utf8_gets_a_parse_error_and_closes(target: Target) {
    let core = target.start().await;
    let mut client = core.client().await;
    client
        .send_bytes(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\"}\n")
        .await;
    expect_parse_error_then_close(&mut client).await;
}

pub async fn a_connection_without_hello_is_closed_after_the_deadline(target: Target) {
    let core = target
        .start_with(|config| {
            config.handshake_timeout = Duration::from_millis(300);
        })
        .await;

    let mut silent = Client::connect(&core.endpoint).await;
    let mut greeted = core.client().await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    assert_eq!(silent.read_line().await, None, "closed without a word");
    // The deadline only applies before the handshake.
    greeted.result("core.status", json!({})).await;
}
