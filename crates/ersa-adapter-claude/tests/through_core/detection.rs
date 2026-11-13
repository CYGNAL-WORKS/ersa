//! Detection of Claude Code through Core's registry: the version check runs
//! with the adapter's own environment.

use std::sync::Arc;

use ersa_adapter::{Adapter, ProcessSpec};
use ersa_core::adapters::detect;

// verifies: PROTO-CLAUDE-CODE-STREAM-R19
#[tokio::test]
async fn the_version_check_runs_with_the_adapters_environment() {
    let transport = ersa_adapter::fake::FakeTransport::new(["2.1.316 (Claude Code)"]);
    let claude: Arc<dyn Adapter> = Arc::new(ersa_adapter_claude::ClaudeAdapter::new());
    let found = detect(&transport, None, std::slice::from_ref(&claude)).await;
    assert_eq!(found[0].version.as_deref(), Some("2.1.316"));
    // The tested range and the choices are the adapter's own.
    let info = found[0].info();
    assert_eq!(
        (info.tested.min.as_str(), info.tested.max.as_str()),
        (
            ersa_adapter_claude::MIN_TESTED_VERSION,
            ersa_adapter_claude::MAX_TESTED_VERSION
        )
    );
    assert_eq!(info.models, claude.choices().models);
    assert_eq!(info.label, "Claude Code");

    let mut session = ProcessSpec::default();
    claude.set_environment(&mut session);
    let check = &transport.spawned()[0];
    assert_eq!(check.args, ["--version"]);
    assert_eq!(check.env, session.env);
    assert_eq!(check.env_remove, session.env_remove);
    assert!(
        check
            .env_remove
            .iter()
            .any(|n| n == "CLAUDE_CODE_SESSION_ID")
    );
}
