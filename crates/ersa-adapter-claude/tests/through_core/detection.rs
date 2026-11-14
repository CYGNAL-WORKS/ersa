//! Detection of Claude Code through Core's registry: the version check runs
//! with the adapter's own environment, and a CLI found is asked for its
//! models.

use std::sync::Arc;

use ersa_adapter::fake::FakeTransport;
use ersa_adapter::{Adapter, ProcessSpec};
use ersa_core::adapters::detect;
use ersa_protocol::core_api::BackendState;
use serde_json::Value;

use crate::common::fixture;

/// The version line Claude Code 2.1.316 prints.
fn version_check() -> FakeTransport {
    FakeTransport::new(["2.1.316 (Claude Code)"])
}

/// A probe CLI that answers `initialize` with fixture 25's answer, which
/// lists the models, and exits when its stdin closes.
fn probe() -> FakeTransport {
    let answer = fixture("2.1.316/25-controls-between-turns")
        .stdout
        .into_iter()
        .find(|line| {
            serde_json::from_str::<Value>(line).unwrap()["response"]["response"]["models"]
                .is_array()
        })
        .unwrap();
    FakeTransport::new(Vec::<String>::new())
        .wait_for_stdin(|_| Ok(()))
        .then_stdout([answer])
        .hold_open()
}

fn claude() -> Arc<dyn Adapter> {
    Arc::new(ersa_adapter_claude::ClaudeAdapter::new())
}

// verifies: PROTO-CLAUDE-CODE-STREAM-R19
#[tokio::test]
async fn the_version_check_runs_with_the_adapters_environment() {
    let transport = version_check();
    let claude = claude();
    let found = detect(
        &transport,
        Arc::new(probe()),
        None,
        std::slice::from_ref(&claude),
    )
    .await;
    assert_eq!(found[0].version.as_deref(), Some("2.1.316"));
    // The tested range is the adapter's own.
    let info = found[0].info();
    assert_eq!(
        (info.tested.min.as_str(), info.tested.max.as_str()),
        (
            ersa_adapter_claude::MIN_TESTED_VERSION,
            ersa_adapter_claude::MAX_TESTED_VERSION
        )
    );
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

/// A CLI found lists the models it reports, its `default` entry marked; one
/// that cannot answer is `error` with no models, its version kept.
// verifies: PROTO-CORE-API-R72
// verifies: PROTO-CLAUDE-CODE-STREAM-R39
#[tokio::test]
async fn a_cli_found_lists_the_models_it_reports() {
    let probe = probe();
    let found = detect(&version_check(), Arc::new(probe.clone()), None, &[claude()]).await;
    let info = found[0].info();
    assert_eq!(info.status, BackendState::Available);
    assert_eq!(info.models.len(), 14);
    assert_eq!(
        (info.models[0].id.as_str(), info.models[0].default),
        ("default", true)
    );
    assert_eq!(info.models.iter().filter(|m| m.default).count(), 1);
    // The probe ran with the adapter's environment too.
    let mut session = ProcessSpec::default();
    claude().set_environment(&mut session);
    assert_eq!(probe.spawned()[0].env, session.env);

    let silent = FakeTransport::new(Vec::<String>::new()).with_exit_code(1);
    let found = detect(&version_check(), Arc::new(silent), None, &[claude()]).await;
    let info = found[0].info();
    assert_eq!(info.status, BackendState::Error);
    assert!(info.models.is_empty());
    assert_eq!(info.version.as_deref(), Some("2.1.316"));
}
