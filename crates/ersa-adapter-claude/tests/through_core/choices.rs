//! What `backend.list` offers for Claude Code, through Core's API.

use std::sync::Arc;

use ersa_adapter::Adapter;
use serde_json::{Value, json};

use crate::support::{start_with_adapters, tempdir};

/// The efforts and permission modes are the adapter's knowledge, listed
/// even while the CLI is missing from the host; the models are the CLI's
/// own, so a missing CLI lists none (PROTO-CORE-API-R72). No effort is
/// marked, as the CLI marks none. The permission modes are named and
/// ordered as Claude Desktop's permission menu, with `default` marked.
// verifies: PROTO-CORE-API-R72
// verifies: PROTO-CLAUDE-CODE-STREAM-R80
#[tokio::test(flavor = "multi_thread")]
async fn backend_list_offers_claude_codes_choices_with_their_defaults() {
    let claude: Arc<dyn Adapter> = Arc::new(ersa_adapter_claude::ClaudeAdapter::new());
    let core = start_with_adapters(tempdir(), |_| {}, vec![claude]).await;
    let mut client = core.client().await;
    // Checked against the schema on arrival, as every response is.
    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    let claude = &infos[0];
    assert_eq!(claude["label"], json!("Claude Code"));
    assert_eq!(claude["status"], json!("missing"));
    assert_eq!(claude["models"], json!([]));
    let offered = |list: &str| -> Vec<(String, bool)> {
        claude[list]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["id"].as_str().unwrap().to_string(),
                    c["default"] == json!(true),
                )
            })
            .collect()
    };
    let with_default = |ids: &[&str], default: &str| -> Vec<(String, bool)> {
        ids.iter()
            .map(|id| (id.to_string(), *id == default))
            .collect()
    };
    assert_eq!(
        offered("efforts"),
        with_default(&["low", "medium", "high", "xhigh", "max"], "")
    );
    assert_eq!(
        offered("permission_modes"),
        with_default(&["auto", "default", "acceptEdits", "plan"], "default")
    );
    let labels: Vec<&str> = claude["permission_modes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["Auto", "Manual", "Accept edits", "Plan"]);
}

/// Each permission mode's sentence reaches the Interface through Core's
/// `backend.list` on its own mode, and no effort carries one.
// verifies: PROTO-CLAUDE-CODE-STREAM-R81
// verifies: PROTO-CORE-API-R142
#[tokio::test(flavor = "multi_thread")]
async fn backend_list_says_what_each_of_claude_codes_permission_modes_does() {
    let claude: Arc<dyn Adapter> = Arc::new(ersa_adapter_claude::ClaudeAdapter::new());
    let core = start_with_adapters(tempdir(), |_| {}, vec![claude]).await;
    let mut client = core.client().await;
    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    let described: Vec<(String, Value)> = infos[0]["permission_modes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap().to_string(),
                c["description"].clone(),
            )
        })
        .collect();
    let expected = |pairs: &[(&str, &str)]| -> Vec<(String, Value)> {
        pairs
            .iter()
            .map(|&(id, d)| (id.to_string(), json!(d)))
            .collect()
    };
    assert_eq!(
        described,
        expected(&[
            ("auto", "Claude handles permission decisions"),
            ("default", "Always ask before making changes"),
            ("acceptEdits", "Automatically accept all file edits"),
            ("plan", "Create a plan before making changes"),
        ])
    );
    let efforts = infos[0]["efforts"].as_array().unwrap();
    assert!(
        efforts.iter().all(|e| e.get("description").is_none()),
        "{efforts:?}"
    );
}
