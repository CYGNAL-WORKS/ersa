//! Where Core looks for a backend's command, and how long it keeps what it
//! found (PROTO-CORE-API, Finding a backend; `backend.list`).
//!
//! The stand-in backend is a small script in a folder of the test's own,
//! which is on no path Core was started with. The stored search path, which
//! on Windows Core reads from the registry, is the test's own too, so a
//! test can put the folder on it, as an installer does, while Core runs.

mod support;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ersa_core::search_path::StoredEnvironment;
use ersa_core::{CoreConfig, StoredEnvironmentSource};
use serde_json::{Value, json};
use support::fake::{BACKEND, FakeAdapter, Printed};
use support::{Client, start_with_adapters, tempdir};

/// A stored search path of the test's own: the system's Path is empty and
/// the user's holds what the test puts there.
#[derive(Default)]
struct Stored {
    user_path: Mutex<String>,
    /// Holds every read of the stored values until let go, as a folder on
    /// a share that stopped answering would hold a look.
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    reads: AtomicUsize,
}

impl Stored {
    /// Puts `folder` on the user's stored Path, as an installer does.
    fn add(&self, folder: &Path) {
        let mut path = self.user_path.lock().unwrap();
        let mut folders: Vec<PathBuf> = std::env::split_paths(&*path)
            .filter(|f| !f.as_os_str().is_empty())
            .collect();
        folders.push(folder.to_path_buf());
        *path = std::env::join_paths(folders)
            .unwrap()
            .to_string_lossy()
            .into_owned();
    }
}

impl StoredEnvironment for Stored {
    fn paths(&self) -> std::io::Result<Vec<String>> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if let Some(hold) = self.hold.lock().unwrap().take() {
            // Until the test lets go, or forever when it never does.
            let _ = hold.recv();
        }
        Ok(vec![String::new(), self.user_path.lock().unwrap().clone()])
    }

    fn variable(&self, _name: &str) -> Option<String> {
        None
    }
}

/// A name for a stand-in no other test or machine has.
fn unique_name() -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    format!(
        "ersa-test-standin-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    )
}

/// Writes the stand-in `name` into `folder`: it prints a version, then the
/// search path it was given.
fn install(folder: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        let file = folder.join(format!("{name}.cmd"));
        std::fs::write(&file, "@echo off\r\necho 1.0.0\r\necho %PATH%\r\n").unwrap();
        file
    } else {
        let file = folder.join(name);
        std::fs::write(&file, "#!/bin/sh\necho 1.0.0\necho \"$PATH\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        file
    }
}

/// A fake backend that is the command `name`.
fn standin(backend: &'static str, name: &str) -> FakeAdapter {
    let mut fake = FakeAdapter::named(backend);
    fake.command = Some(name.to_string());
    fake
}

/// The configuration: backends detected by their own commands, along the
/// test's stored search path.
fn along(stored: &Arc<Stored>) -> impl FnOnce(&mut CoreConfig) + use<> {
    let stored = Arc::clone(stored);
    move |config: &mut CoreConfig| {
        support::detecting(config);
        config.stored_environment = StoredEnvironmentSource(stored);
    }
}

async fn status_in_backend_list(client: &mut Client, name: &str) -> Value {
    let infos = client
        .result("backend.list", json!({ "host_id": "local" }))
        .await;
    let info = infos
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == json!(name))
        .unwrap_or_else(|| panic!("{name} in {infos}"));
    info["status"].clone()
}

async fn status_in_host_list(client: &mut Client, name: &str) -> Value {
    let hosts = client.result("host.list", json!({})).await;
    let backends = hosts[0]["backends"].as_array().unwrap();
    let backend = backends
        .iter()
        .find(|b| b["name"] == json!(name))
        .unwrap_or_else(|| panic!("{name} in {hosts}"));
    backend["status"].clone()
}

/// The path the stand-in printed each time it ran for `why`.
fn printed_for(printed: &Printed, why: &str) -> Vec<String> {
    printed
        .lock()
        .unwrap()
        .iter()
        .filter(|(w, _)| w == why)
        .map(|(_, line)| line.clone())
        .collect()
}

/// Asserts `path` is the path Core was started with and then `folder`, as
/// the started process was given it.
fn assert_composed(path: &str, folder: &Path) {
    let own = std::env::var("PATH").unwrap_or_default();
    let mut expected: Vec<PathBuf> = std::env::split_paths(&own).collect();
    expected.push(folder.to_path_buf());
    let given: Vec<PathBuf> = std::env::split_paths(path).collect();
    assert_eq!(given, expected, "Core's own path, then the folder: {path}");
}

/// A backend installed into a folder that is on the stored user Path and
/// not on Core's own is found by the very next request, without Core
/// starting again, and the version command, the probe for models and the
/// session's start are each given the whole path as their own.
// verifies: PROTO-CORE-API-R148
// verifies: PROTO-CORE-API-R150
#[tokio::test(flavor = "multi_thread")]
async fn a_backend_installed_on_the_stored_path_while_core_runs_is_found_and_started_with_it() {
    let folder = tempdir();
    let name = unique_name();
    let stored = Arc::new(Stored::default());
    let fake = standin(BACKEND, &name);
    let printed = Arc::clone(&fake.printed);
    let core = start_with_adapters(tempdir(), along(&stored), vec![Arc::new(fake)]).await;
    let mut client = core.client().await;

    // Not installed: missing, in both lists.
    assert_eq!(
        status_in_backend_list(&mut client, BACKEND).await,
        json!("missing")
    );
    assert_eq!(
        status_in_host_list(&mut client, BACKEND).await,
        json!("missing")
    );

    // Installed into a folder that is not yet on any path: still missing.
    install(folder.path(), &name);
    assert_eq!(
        status_in_backend_list(&mut client, BACKEND).await,
        json!("missing")
    );

    // The installer puts the folder on the user's stored Path. The next
    // request finds it, well within the 30 seconds an `error` is kept.
    stored.add(folder.path());
    assert_eq!(
        status_in_host_list(&mut client, BACKEND).await,
        json!("available")
    );
    assert_eq!(
        status_in_backend_list(&mut client, BACKEND).await,
        json!("available")
    );

    let version = printed_for(&printed, "version");
    assert_eq!(version.len(), 1, "detected once: {version:?}");
    assert_composed(&version[0], folder.path());
    let probe = printed_for(&printed, "probe");
    assert_eq!(probe.len(), 1, "{probe:?}");
    assert_composed(&probe[0], folder.path());

    let workspace_id = support::workspace(&mut client, core.dir.path()).await;
    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": BACKEND, "model": "model-a" }),
        )
        .await;
    let deadline = Instant::now() + Duration::from_secs(30);
    let start = loop {
        let start = printed_for(&printed, "start");
        if !start.is_empty() {
            break start;
        }
        assert!(
            Instant::now() < deadline,
            "the session's start ran the stand-in"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_composed(&start[0], folder.path());
}

/// While one backend is available and another missing, no request starts
/// the available one's command again, and each looks for the missing one
/// again, finding it once it is installed. The reuse of a detection in
/// `error` is turned off, so only the keeping of `available` can hold the
/// first one.
// verifies: PROTO-CORE-API-R149
// verifies: PROTO-CORE-API-R150
#[tokio::test(flavor = "multi_thread")]
async fn an_available_backend_is_not_detected_again_because_another_is_missing() {
    let folder = tempdir();
    let name = unique_name();
    let stored = Arc::new(Stored::default());
    stored.add(folder.path());
    let present = FakeAdapter::named("present");
    let present_stats = Arc::clone(&present.stats);
    let absent = standin("absent", &name);
    let configure = {
        let along = along(&stored);
        move |config: &mut CoreConfig| {
            along(config);
            config.failed_detection_ttl = Duration::ZERO;
        }
    };
    let core = start_with_adapters(
        tempdir(),
        configure,
        vec![Arc::new(present), Arc::new(absent)],
    )
    .await;
    let mut client = core.client().await;

    for _ in 0..2 {
        assert_eq!(
            status_in_backend_list(&mut client, "present").await,
            json!("available")
        );
        assert_eq!(
            status_in_backend_list(&mut client, "absent").await,
            json!("missing")
        );
    }
    assert_eq!(
        status_in_host_list(&mut client, "absent").await,
        json!("missing")
    );
    let workspace_id = support::workspace(&mut client, core.dir.path()).await;
    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace_id, "backend": "present", "model": "model-a" }),
        )
        .await;
    let reads_before = stored.reads.load(Ordering::SeqCst);

    install(folder.path(), &name);
    assert_eq!(
        status_in_host_list(&mut client, "absent").await,
        json!("available")
    );
    assert!(
        stored.reads.load(Ordering::SeqCst) > reads_before,
        "looked again"
    );
    assert_eq!(
        status_in_backend_list(&mut client, "present").await,
        json!("available")
    );

    assert_eq!(
        present_stats.model_probes.load(Ordering::SeqCst),
        1,
        "the available backend was asked for its models once"
    );
}

/// A look that has not ended within its limit ends with the backend in
/// `error`, and the limit is 10 seconds unless a test shortens it.
// verifies: PROTO-CORE-API-R151
#[tokio::test(flavor = "multi_thread")]
async fn a_look_that_does_not_end_in_time_leaves_the_backend_in_error() {
    let defaults = CoreConfig::new(std::env::temp_dir()).unwrap();
    assert_eq!(defaults.backend_look_limit, Duration::from_secs(10));

    let stored = Arc::new(Stored::default());
    let (let_go, hold) = std::sync::mpsc::channel();
    *stored.hold.lock().unwrap() = Some(hold);
    let configure = {
        let along = along(&stored);
        move |config: &mut CoreConfig| {
            along(config);
            config.backend_look_limit = Duration::from_millis(300);
        }
    };
    let fake = standin(BACKEND, &unique_name());
    let core = start_with_adapters(tempdir(), configure, vec![Arc::new(fake)]).await;
    let mut client = core.client().await;

    let asked = Instant::now();
    assert_eq!(
        status_in_backend_list(&mut client, BACKEND).await,
        json!("error")
    );
    assert!(
        asked.elapsed() < Duration::from_secs(10),
        "answered at the limit, not when the look ended: {:?}",
        asked.elapsed()
    );
    let _ = let_go.send(());
}

/// A command named by its full path is started as named: one that is there
/// and cannot start (here a folder) lists as `error`, and one that is not
/// there as `missing`.
// verifies: PROTO-CORE-API-R148
#[tokio::test(flavor = "multi_thread")]
async fn a_command_named_by_its_full_path_is_started_as_named() {
    let folder = tempdir();
    let stored = Arc::new(Stored::default());
    let cannot_start = standin("cannot-start", &folder.path().display().to_string());
    let absent = standin(
        "absent",
        &folder
            .path()
            .join("ersa-no-such-program")
            .display()
            .to_string(),
    );
    let core = start_with_adapters(
        tempdir(),
        along(&stored),
        vec![Arc::new(cannot_start), Arc::new(absent)],
    )
    .await;
    let mut client = core.client().await;
    assert_eq!(
        status_in_backend_list(&mut client, "cannot-start").await,
        json!("error")
    );
    assert_eq!(
        status_in_backend_list(&mut client, "absent").await,
        json!("missing")
    );
}
