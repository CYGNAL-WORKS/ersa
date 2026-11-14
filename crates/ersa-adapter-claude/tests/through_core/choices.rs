//! What `backend.list` offers for Claude Code, through Core's API.

use std::sync::Arc;

use ersa_adapter::Adapter;
use serde_json::json;

use crate::support::{start_with_adapters, tempdir};

/// The efforts and permission modes are the adapter's knowledge, listed
/// even while the CLI is missing from the host; the models are the CLI's
/// own, so a missing CLI lists none (PROTO-CORE-API-R72). No effort is
/// marked, as the CLI marks none.
// verifies: PROTO-CORE-API-R72
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
        with_default(&["default", "acceptEdits", "plan", "auto"], "default")
    );
}
