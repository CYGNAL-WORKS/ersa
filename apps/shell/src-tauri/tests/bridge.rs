//! The bridge against a real `ersa-core serve` that the test launches through
//! the Shell's own launcher: launch, handshake, requests, attach again after
//! the Shell lets go, and shutdown.
//!
//! Needs the `ersa-core` executable: `ERSA_CORE_BIN`, or the root workspace's
//! debug build (`cargo build -p ersa-cli` at the repository root).
//!
//! Each test's Core serves an endpoint of its own (`serve --endpoint`), so
//! the tests never reach or stop the user's own Core.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use ersa_shell::bridge::{BridgeError, CoreLink, LinkEvent, LinkOptions};
use ersa_shell::launch::{self, CoreLocation, How};
use serde_json::{Value, json};
use tokio::sync::mpsc;

fn core_program() -> PathBuf {
    let program = std::env::var_os("ERSA_CORE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // CARGO_MANIFEST_DIR is apps/shell/src-tauri.
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../target/debug")
                .join(format!("ersa-core{}", std::env::consts::EXE_SUFFIX))
        });
    assert!(
        program.exists(),
        "no ersa-core at {}: run `cargo build -p ersa-cli` at the repository root, or set ERSA_CORE_BIN",
        program.display()
    );
    program
}

/// A Core location private to this test: an endpoint labelled with the
/// process id and a counter, which `serve` is told to use, and a data
/// directory in `dir`.
fn private_location(dir: &Path) -> CoreLocation {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let label = format!(
        "shell-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let endpoint = ersa_core::platform::labelled_endpoint(&label).expect("an endpoint");
    CoreLocation {
        program: core_program(),
        endpoint: endpoint.clone(),
        serve_args: vec![
            "--endpoint".into(),
            endpoint.into(),
            "--data-dir".into(),
            dir.join("data").into_os_string(),
        ],
        env: Vec::new(),
        log_file: Some(dir.join("core.log")),
        start_timeout: Duration::from_secs(60),
    }
}

async fn open(location: &CoreLocation) -> (CoreLink, How, mpsc::UnboundedReceiver<LinkEvent>) {
    try_open(location)
        .await
        .unwrap_or_else(|e| panic!("attach, launch or handshake failed: {e}"))
}

#[tokio::test]
async fn launches_core_then_attaches_and_round_trips_requests() {
    let dir = tempfile::tempdir().expect("temp dir");
    let location = private_location(dir.path());

    let (link, how, _events) = open(&location).await;
    assert_eq!(
        how,
        How::Launched,
        "{} is this test's alone",
        location.endpoint
    );

    // Run the checks as their own task so that, if one fails, the Core this
    // test launched is still stopped rather than left running.
    let checks = tokio::spawn(round_trips(link, location.clone()));
    if let Err(failed) = checks.await {
        if let Ok((link, _, _)) = try_open(&location).await {
            let _ = link.request("core.shutdown", None).await;
        }
        std::panic::resume_unwind(failed.into_panic());
    }
}

async fn try_open(
    location: &CoreLocation,
) -> Result<(CoreLink, How, mpsc::UnboundedReceiver<LinkEvent>), String> {
    let (stream, how) = launch::attach_or_launch(location)
        .await
        .map_err(|e| e.to_string())?;
    let (events, received) = mpsc::unbounded_channel();
    let options = LinkOptions {
        probe_endpoint: Some(location.endpoint.clone()),
        timeout: None,
    };
    let link = CoreLink::open(stream, options, move |event| {
        let _ = events.send(event);
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok((link, how, received))
}

async fn round_trips(link: CoreLink, location: CoreLocation) {
    // The handshake: Core's `core.hello` result, as the Interface receives it.
    let hello = link.hello();
    assert_eq!(hello["api_version"], json!(1));
    assert!(hello["core_version"].is_string(), "{hello}");
    assert!(hello["host"]["id"].is_string(), "{hello}");

    let status = link
        .request("core.status", None)
        .await
        .expect("core.status");
    assert!(status["uptime_s"].is_u64(), "{status}");
    let projects = link
        .request("project.list", None)
        .await
        .expect("project.list");
    assert_eq!(projects, json!([]));

    // Core's error objects come through unchanged.
    let error = link
        .request("project.remove", Some(json!({ "id": "no-such-project" })))
        .await
        .expect_err("removing an unknown project fails");
    let BridgeError::Rpc { error } = error else {
        panic!("expected Core's error, got {error:?}");
    };
    assert_eq!(error.code, -32000);

    // Letting go of the connection (the window closing) leaves Core running:
    // the next attach finds it.
    drop(link);
    let (link, how, mut events) = open(&location).await;
    assert_eq!(how, How::Attached);
    let projects = link
        .request("project.list", None)
        .await
        .expect("project.list");
    assert_eq!(projects, json!([]));

    // Stop the Core this test started. Core answers, then closes.
    link.request("core.shutdown", None)
        .await
        .expect("core.shutdown");
    let closed = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match events.recv().await {
                Some(LinkEvent::Closed { reason }) => return reason,
                Some(_) => continue,
                None => panic!("the event channel closed without a Closed event"),
            }
        }
    })
    .await
    .expect("the connection closes after core.shutdown");
    assert!(!closed.is_empty());
    assert!(link.is_closed());
    let after = link.request("core.status", None).await;
    assert!(
        matches!(after, Err(BridgeError::Unavailable { .. })),
        "{after:?}"
    );
}

#[tokio::test]
async fn a_core_that_cannot_start_is_reported_with_its_output() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut location = private_location(dir.path());
    // An argument `serve` does not take: clap prints an error and exits.
    location.serve_args = vec!["--no-such-flag".into()];
    // An earlier run's output is in the shared log; the error must not show it.
    let log = location.log_file.clone().expect("a log file");
    std::fs::write(&log, "a line from an earlier run\n").expect("seed the log");
    let error = launch::attach_or_launch(&location)
        .await
        .err()
        .expect("Core cannot start");
    let message = error.to_string();
    assert!(message.contains("exited"), "{message}");
    assert!(message.contains("--no-such-flag"), "{message}");
    assert!(!message.contains("earlier run"), "{message}");
}

#[test]
fn values_round_trip_as_json() {
    // BridgeError reaches the Interface as JSON; its shape is part of the
    // bridge's contract with the Interface's client.
    let error = BridgeError::unavailable("gone");
    let value: Value = serde_json::to_value(&error).expect("serializes");
    assert_eq!(value, json!({ "kind": "unavailable", "message": "gone" }));
}
