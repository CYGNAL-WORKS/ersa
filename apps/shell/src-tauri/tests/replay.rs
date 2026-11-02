//! What the Interface receives from the replay server, kept as captures the
//! Interface's own tests replay (issue #14, Test seams: against the replay
//! server). Each test serves one recording with `ersa-core replay --pace
//! instant` on an endpoint of its own, subscribes as the Interface does, and
//! compares the notifications it gets with the committed capture in
//! `apps/shell/src/session/replays/`. When Core's projection changes on
//! purpose, rewrite the captures with `ERSA_UPDATE_REPLAYS=1` and review the
//! diff: the Interface's tests then run against the new stream.
//!
//! Needs the `ersa-core` executable, as the bridge tests do. Updating the
//! captures builds it first, so a capture never comes from a stale binary,
//! and each capture names the Core commit it came from (`core`), which the
//! comparison ignores.

mod common;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::{core_program, repo_root, unique_label};
use ersa_shell::bridge::{CoreLink, LinkEvent, LinkOptions};
use ersa_shell::launch::{self, CoreLocation, How, LaunchError};
use serde_json::{Value, json};
use tokio::sync::mpsc;

/// How long the replay server may take to start serving.
const START: Duration = Duration::from_secs(60);
/// After this long with nothing arriving, the test asks Core whether the
/// replay has played out. Silence alone never ends the capture.
const QUIET: Duration = Duration::from_millis(500);
/// A bound on the whole collection, so a replay that never ends fails.
const COLLECT: Duration = Duration::from_secs(60);
/// How long the replay server may take to exit after `core.shutdown`.
const EXIT: Duration = Duration::from_secs(15);

/// The replay server process, stopped when the test ends however it ends.
/// A test that finishes sends `core.shutdown`, so the server removes its
/// temporary data directory itself; this waits for that, and kills the
/// server only if it has not exited by then (a test that panicked first).
/// It is this test's own child, started without detaching, so killing it
/// stops it.
struct ReplayServer(Child);

impl Drop for ReplayServer {
    fn drop(&mut self) {
        // A panicking test sent no shutdown: stop the server at once.
        if std::thread::panicking() {
            let _ = self.0.kill();
            let _ = self.0.wait();
            return;
        }
        let deadline = std::time::Instant::now() + EXIT;
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = self.0.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fixture(name: &str) -> PathBuf {
    repo_root()
        .join("docs/briefs/protocol/fixtures/claude-code/2.1.302")
        .join(format!("{name}.jsonl"))
}

fn capture_path(name: &str) -> PathBuf {
    repo_root()
        .join("apps/shell/src/session/replays")
        .join(format!("{name}.json"))
}

/// Serves `name` and returns what the Interface sees of it: the session
/// listing on attach, every `session.event` notification as the Interface's
/// channel receives it (`{ kind: "notification", method, params }`) until
/// the replay goes quiet, and the listing after that. `session.tile`
/// notifications are left out: Core sends them as the tile changes, and at
/// instant pace how many it sends, and where they fall among the events,
/// varies from run to run; the listing after shows the tile they led to.
async fn received(name: &str) -> Value {
    let label = unique_label("shell-replay");
    let _server = ReplayServer(
        Command::new(core_program())
            .arg("replay")
            .arg("--fixture")
            .arg(fixture(name))
            .args(["--pace", "instant", "--endpoint", &label])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start ersa-core replay"),
    );

    // Attach as a debug window does with ERSA_ATTACH: to the label only,
    // never launching Core there.
    let location = CoreLocation::attach_only(&label).expect("a location");
    let stream = tokio::time::timeout(START, async {
        loop {
            match launch::attach_or_launch(&location).await {
                Ok((stream, how)) => {
                    assert_eq!(how, How::Attached);
                    return stream;
                }
                Err(LaunchError::NotServing { .. }) => {
                    tokio::time::sleep(Duration::from_millis(50)).await
                }
                Err(e) => panic!("cannot attach to the replay server: {e}"),
            }
        }
    })
    .await
    .expect("the replay server serves its endpoint");
    let (events, mut incoming) = mpsc::unbounded_channel();
    let link = CoreLink::open(stream, LinkOptions::default(), move |event| {
        let _ = events.send(event);
    })
    .await
    .expect("handshake with the replay server");

    let sessions = link
        .request("session.list", None)
        .await
        .expect("session.list");
    let id = sessions[0]["id"]
        .as_str()
        .expect("a session id")
        .to_string();
    link.request(
        "session.subscribe",
        Some(json!({ "id": id, "from_seq": 0 })),
    )
    .await
    .expect("session.subscribe");

    // How many lines the replay feeds into the raw log: the recording's
    // backend output and what was written to the backend.
    let recorded = ersa_core::replay::Recording::load(&fixture(name))
        .expect("the recording loads")
        .into_lines()
        .len();
    let mut notifications = Vec::new();
    let mut last_seq: Option<u64> = None;
    let after = tokio::time::timeout(COLLECT, async {
        loop {
            match tokio::time::timeout(QUIET, incoming.recv()).await {
                Ok(Some(LinkEvent::Notification { method, params }))
                    if method == "session.event" =>
                {
                    last_seq = params["event"]["seq"].as_u64();
                    notifications.push(
                        json!({ "kind": "notification", "method": method, "params": params }),
                    );
                }
                Ok(Some(LinkEvent::Notification { .. })) => {}
                Ok(Some(LinkEvent::Closed { reason })) => {
                    panic!("the replay closed the connection: {reason}")
                }
                Ok(None) => panic!("the link's event channel closed"),
                // Quiet: the capture is complete once every recorded line
                // is in the session's raw log (Core projects a line as it
                // takes it in) and Core lists no event past the last one
                // received. Silence alone never ends it.
                Err(_) => {
                    let after = link
                        .request("session.list", None)
                        .await
                        .expect("session.list");
                    let raw = link
                        .request("session.raw_log", Some(json!({ "id": id })))
                        .await
                        .expect("session.raw_log");
                    let fed = raw.as_array().expect("raw log entries").len();
                    if fed == recorded && after[0]["last_event_seq"].as_u64() == last_seq {
                        return after;
                    }
                }
            }
        }
    })
    .await
    .expect("the replay plays out");
    // Stop the server as an Interface would, so it cleans up after itself.
    link.request("core.shutdown", None)
        .await
        .expect("core.shutdown");
    normalise(
        json!({ "sessions": sessions, "notifications": notifications, "sessions_after": after }),
        &id,
    )
}

/// Replaces what differs from run to run (identifiers Core issues, times it
/// records, its temporary data directory) with fixed stand-ins, so a
/// capture changes only when the stream does.
fn normalise(mut value: Value, session_id: &str) -> Value {
    fn walk(value: &mut Value, session_id: &str) {
        match value {
            Value::Object(map) => {
                for (key, field) in map.iter_mut() {
                    match (key.as_str(), &*field) {
                        ("ts" | "created_at", Value::String(_)) => {
                            *field = json!("2026-01-01T00:00:00Z")
                        }
                        ("workspace_id", Value::String(_)) => *field = json!("w-replay"),
                        ("cwd", Value::String(_)) => *field = json!("/replay"),
                        (_, Value::String(s)) if s == session_id => *field = json!("s-replay"),
                        _ => walk(field, session_id),
                    }
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|item| walk(item, session_id)),
            _ => {}
        }
    }
    walk(&mut value, session_id);
    value
}

/// Builds `ersa-core` from the current sources, once per test run, so the
/// captures about to be rewritten come from this Core and not an older
/// binary left in `target/`. An `ERSA_CORE_BIN` override names a binary this
/// test cannot rebuild, so updating refuses it.
/// Returns the commit stamp for the captures, taken at the same moment, so
/// both tests of a run name the same Core.
fn build_core() -> &'static str {
    static BUILT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BUILT.get_or_init(|| {
        assert!(
            std::env::var_os("ERSA_CORE_BIN").is_none(),
            "ERSA_UPDATE_REPLAYS rebuilds ersa-core itself; unset ERSA_CORE_BIN"
        );
        // Cargo sets CARGO for the tests it runs: the same toolchain.
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .args(["build", "-p", "ersa-cli"])
            .current_dir(repo_root())
            .status()
            .expect("run cargo build");
        assert!(status.success(), "cargo build -p ersa-cli failed");
        core_commit()
    })
}

/// The commit a capture comes from, marked `-dirty` when the working tree
/// has uncommitted changes outside the captures themselves (which this run
/// rewrites), so a stale capture can be traced.
fn core_commit() -> String {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(repo_root())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let head = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    match git(&[
        "status",
        "--porcelain",
        "--",
        ".",
        ":!apps/shell/src/session/replays",
    ]) {
        Some(changes) if !changes.is_empty() => format!("{head}-dirty"),
        _ => head,
    }
}

async fn check(name: &str) {
    let stamp = std::env::var_os("ERSA_UPDATE_REPLAYS").map(|_| build_core());
    let actual = received(name).await;
    let path = capture_path(name);
    let pretty = serde_json::to_string_pretty(&actual).expect("serializes") + "\n";
    if let Some(stamp) = stamp {
        let mut written = actual.clone();
        written["core"] = json!(stamp);
        let written = serde_json::to_string_pretty(&written).expect("serializes") + "\n";
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("create the folder");
        std::fs::write(&path, written).expect("write the capture");
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    let mut committed: Value = serde_json::from_str(&committed).unwrap_or(Value::Null);
    // Where the capture came from is provenance, not part of the stream.
    if let Some(map) = committed.as_object_mut() {
        map.remove("core");
    }
    assert!(
        committed == actual,
        "{} is not what the replay server sends now; if Core changed on purpose, run with ERSA_UPDATE_REPLAYS=1 and review the diff. Received:\n{pretty}",
        path.display()
    );
}

// verifies: ARCH-CORE-R11
#[tokio::test]
async fn streamed_text_capture_is_current() {
    check("02-partial-messages").await;
}

// verifies: ARCH-CORE-R11
#[tokio::test]
async fn permission_capture_is_current() {
    check("11-stream-permission-prompt-tool-stdio").await;
}
