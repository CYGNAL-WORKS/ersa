//! The bridge against a real `ersa-core serve` that the test launches through
//! the Shell's own launcher: launch, handshake, requests, attach again after
//! the Shell lets go, and shutdown.
//!
//! Needs the `ersa-core` executable: `ERSA_CORE_BIN`, or the root workspace's
//! debug build (`cargo build -p ersa-cli` at the repository root).
//!
//! Each test's Core serves an endpoint of its own (`serve --endpoint`), so
//! the tests never reach or stop the user's own Core.

mod common;

use std::time::Duration;

use common::{private_location, unique_label};
use ersa_shell::bridge::{BridgeError, CoreLink, LinkEvent, LinkOptions};
use ersa_shell::launch::{self, CoreLocation, How};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// Bounds on launching and opening a link, and on stopping a leftover Core.
/// Above the location's 60 s start timeout, so that one reports first.
const OPEN: Duration = Duration::from_secs(90);
const CLEANUP: Duration = Duration::from_secs(30);

async fn open(location: &CoreLocation) -> (CoreLink, How, mpsc::UnboundedReceiver<LinkEvent>) {
    try_open(location)
        .await
        .unwrap_or_else(|e| panic!("attach, launch or handshake failed: {e}"))
}

#[tokio::test]
async fn launches_core_then_attaches_and_round_trips_requests() {
    let dir = tempfile::tempdir().expect("temp dir");
    let location = private_location(dir.path());

    // Run everything from the launch on as its own task so that, however it
    // fails, the Core it launched is stopped afterwards. That Core is
    // detached and outside the test's job, so nothing else would stop it.
    let checks = tokio::spawn({
        let location = location.clone();
        async move {
            let (link, how, _events) = open(&location).await;
            assert_eq!(
                how,
                How::Launched,
                "{} is this test's alone",
                location.endpoint
            );
            round_trips(link, location).await;
        }
    });
    let outcome = checks.await;
    stop_any_core(&location.endpoint).await;
    if let Err(failed) = outcome {
        std::panic::resume_unwind(failed.into_panic());
    }
}

/// Stops whatever Core still serves `endpoint`, if one does, without ever
/// launching one. Bounded, so a Core that accepts but never answers cannot
/// hang the test run.
async fn stop_any_core(endpoint: &str) {
    let stop = async {
        let Ok(stream) = ersa_core::connect(endpoint).await else {
            return; // nobody serves it: the checks stopped their Core
        };
        if let Ok(link) = CoreLink::open(stream, LinkOptions::default(), |_| {}).await {
            let _ = link.request("core.shutdown", None).await;
        }
        // Wait until it has gone, so it no longer holds the data directory
        // the test is about to remove.
        while ersa_core::connect(endpoint).await.is_ok() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    if tokio::time::timeout(CLEANUP, stop).await.is_err() {
        eprintln!("could not stop the Core at {endpoint} within {CLEANUP:?}; it may still run");
    }
}

/// Launches or attaches, then opens the link, within [`OPEN`] in all, so a
/// Core that accepts but never answers fails the test instead of hanging it.
async fn try_open(
    location: &CoreLocation,
) -> Result<(CoreLink, How, mpsc::UnboundedReceiver<LinkEvent>), String> {
    tokio::time::timeout(OPEN, open_link(location))
        .await
        .map_err(|_| format!("no Core answered at {} within {OPEN:?}", location.endpoint))?
}

async fn open_link(
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
    assert_eq!(hello["api_version"], json!(4));
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
    assert!(message.contains("stopped before it was ready"), "{message}");
    assert!(message.contains("--no-such-flag"), "{message}");
    assert!(!message.contains("earlier run"), "{message}");
}

#[tokio::test]
async fn an_attach_only_location_never_launches_core() {
    // What a debug build's ERSA_ATTACH uses: a labelled endpoint someone
    // else serves. With nobody serving it, attaching fails and starts nothing.
    let location = CoreLocation::attach_only(&unique_label("shell-attach")).expect("a location");
    let error = launch::attach_or_launch(&location)
        .await
        .err()
        .expect("nothing serves the label");
    assert!(
        matches!(error, launch::LaunchError::NotServing { .. }),
        "{error}"
    );
    assert!(!ersa_shell::bridge::alive(&location.endpoint).await);
}

#[test]
fn values_round_trip_as_json() {
    // BridgeError reaches the Interface as JSON; its shape is part of the
    // bridge's contract with the Interface's client.
    let error = BridgeError::unavailable("gone");
    let value: Value = serde_json::to_value(&error).expect("serializes");
    assert_eq!(value, json!({ "kind": "unavailable", "message": "gone" }));
    // A request written before it failed says so, as a send that may have reached the agent.
    let value = serde_json::to_value(BridgeError::unanswered("gone")).expect("serializes");
    assert_eq!(
        value,
        json!({ "kind": "unavailable", "message": "gone", "sent": true })
    );
}
