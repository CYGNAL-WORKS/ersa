//! The limits a connection lives under (`PROTO-CORE-API`, Transport and
//! framing): the line cap, UTF-8 only, and the handshake deadline.

mod support;

use std::time::Duration;

use serde_json::{Value, json};
use support::{Client, assert_valid, start_with};

/// Reads the parse error Core sends for an unreadable line, then checks the
/// connection was closed.
async fn expect_parse_error_then_close(client: &mut Client) {
    let response = client.read_line().await.expect("a parse error first");
    assert_valid("core.status", &response);
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], json!(-32700));
    assert_eq!(client.read_line().await, None, "then Core closes");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_line_over_the_cap_gets_a_parse_error_and_closes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let core = start_with(dir, |config| config.max_line_bytes = 1024).await;

    // Exactly at the cap is fine (a valid request padded with spaces).
    let mut client = core.client().await;
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "core.status" }).to_string();
    let padded = format!("{request:<1024}");
    client.send_line(&padded).await;
    let response = client.read_line().await.expect("an answer");
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

#[tokio::test(flavor = "multi_thread")]
async fn a_line_that_is_not_utf8_gets_a_parse_error_and_closes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let core = start_with(dir, |_| {}).await;
    let mut client = core.client().await;
    client
        .send_bytes(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"\xff\"}\n")
        .await;
    expect_parse_error_then_close(&mut client).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_without_hello_is_closed_after_the_deadline() {
    let dir = tempfile::tempdir().expect("temp dir");
    let core = start_with(dir, |config| {
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
