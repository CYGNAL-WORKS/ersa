//! `ersa-core serve --endpoint`: a second Core serves on an endpoint of its
//! own while another serves the default, and clients reach each one there
//! (`PROTO-CORE-API`, Transport and framing).

use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

const START: Duration = Duration::from_secs(60);
const REPLY: Duration = Duration::from_secs(30);

/// A Core this test started. `kill_on_drop` stops it if the test fails
/// before `core.shutdown`.
struct Core {
    child: Child,
    endpoint: String,
}

/// Starts `ersa-core serve` with `args` after `serve` and `env` added, and
/// waits until a client can attach at `endpoint`.
async fn start(dir: &Path, endpoint: String, args: &[&str], env: &[(OsString, OsString)]) -> Core {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ersa-core"))
        .arg("serve")
        .args(args)
        .arg("--data-dir")
        .arg(dir.join("data"))
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("ersa-core starts");
    let deadline = tokio::time::Instant::now() + START;
    while ersa_core::connect(&endpoint).await.is_err() {
        if let Ok(Some(status)) = child.try_wait() {
            panic!("ersa-core exited before serving {endpoint}: {status}");
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "ersa-core did not serve {endpoint} in time"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Core { child, endpoint }
}

/// Attaches to `endpoint`, says `core.hello`, then sends `request` if
/// given. Returns the last reply's result, skipping notifications.
async fn exchange(endpoint: &str, request: Option<&str>) -> Value {
    // `connect` itself refuses a server run by another user, so getting past
    // it means the server was verified as usual.
    let stream = ersa_core::connect(endpoint).await.expect("attach");
    let (read, mut write) = tokio::io::split(stream);
    let mut lines = BufReader::new(read).lines();
    let mut messages = vec![json!({ "jsonrpc": "2.0", "id": 1, "method": "core.hello",
        "params": { "client": "ersa-cli-tests", "client_version": "0", "api_versions": [4] } })];
    if let Some(method) = request {
        messages.push(json!({ "jsonrpc": "2.0", "id": 2, "method": method }));
    }
    let last = json!(messages.len());
    for message in messages {
        let mut bytes = message.to_string().into_bytes();
        bytes.push(b'\n');
        write.write_all(&bytes).await.expect("write");
    }
    loop {
        let line = tokio::time::timeout(REPLY, lines.next_line())
            .await
            .expect("Core answered in time")
            .expect("read")
            .expect("Core keeps the connection open");
        let reply: Value = serde_json::from_str(&line).expect("JSON");
        if reply.get("id").is_none() {
            continue; // a notification
        }
        let result = reply
            .get("result")
            .unwrap_or_else(|| panic!("request {} failed: {reply:#}", reply["id"]));
        if reply["id"] == last {
            return result.clone();
        }
    }
}

impl Core {
    /// `core.hello`'s result: the server, as a client attaching sees it.
    async fn hello(&self) -> Value {
        exchange(&self.endpoint, None).await
    }

    async fn stop(mut self) {
        exchange(&self.endpoint, Some("core.shutdown")).await;
        let status = tokio::time::timeout(REPLY, self.child.wait())
            .await
            .expect("Core exits after core.shutdown")
            .expect("wait");
        assert!(status.success(), "{status}");
    }
}

/// Where this test's Cores serve. On Unix both get the test's private
/// directory as `XDG_RUNTIME_DIR`, so their sockets go there and are removed
/// with it even if a Core is killed. A Windows pipe name has no such
/// override (PROTO-CORE-API: nothing in the environment overrides it).
struct Place {
    env: Vec<(OsString, OsString)>,
    #[cfg(unix)]
    run: std::path::PathBuf,
}

impl Place {
    #[cfg(unix)]
    fn new(dir: &Path) -> Place {
        use std::os::unix::fs::DirBuilderExt;
        let run = dir.join("run");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&run)
            .expect("a private run directory");
        Place {
            env: vec![("XDG_RUNTIME_DIR".into(), run.clone().into_os_string())],
            run,
        }
    }

    #[cfg(windows)]
    fn new(_dir: &Path) -> Place {
        Place { env: Vec::new() }
    }

    /// The user's own endpoint, as a Core started with `env` serves it.
    #[cfg(unix)]
    fn default_endpoint(&self) -> String {
        self.run
            .join("ersa-core.sock")
            .to_string_lossy()
            .into_owned()
    }

    /// The endpoint `serve --endpoint <label>` serves, as a Core started
    /// with `env` names it: on Unix, beside the default socket.
    fn labelled(&self, label: &str) -> String {
        #[cfg(unix)]
        {
            let endpoint = self
                .run
                .join(format!("ersa-core-{label}.sock"))
                .to_string_lossy()
                .into_owned();
            // Same naming as the platform's, only in this test's directory.
            let platform = ersa_core::platform::labelled_endpoint(label).expect("endpoint");
            assert!(
                platform.ends_with(&format!("/ersa-core-{label}.sock")),
                "{platform}"
            );
            endpoint
        }
        #[cfg(windows)]
        {
            ersa_core::platform::labelled_endpoint(label).expect("endpoint")
        }
    }
}

/// The Core playing the user's own. On Unix it serves its true default, in
/// the test's directory. On Windows it serves a labelled endpoint standing
/// in for the default pipe, which this test must not touch.
async fn start_default(dir: &Path, place: &Place) -> Core {
    #[cfg(unix)]
    {
        start(dir, place.default_endpoint(), &[], &place.env).await
    }
    #[cfg(windows)]
    {
        let label = format!("cli-test-default-{}", std::process::id());
        let args = ["--endpoint", label.as_str()];
        start(dir, place.labelled(&label), &args, &place.env).await
    }
}

#[tokio::test]
async fn a_core_on_an_explicit_endpoint_runs_beside_the_default_one() {
    let first = tempfile::tempdir().expect("temp dir");
    let second = tempfile::tempdir().expect("temp dir");
    let place = Place::new(first.path());
    let default = start_default(first.path(), &place).await;

    let label = format!("cli-test-{}", std::process::id());
    let endpoint = place.labelled(&label);
    assert_ne!(endpoint, default.endpoint);
    let explicit = start(second.path(), endpoint, &["--endpoint", &label], &place.env).await;

    // Both answer `core.hello`, each on its own endpoint.
    for core in [&default, &explicit] {
        let hello = core.hello().await;
        assert_eq!(hello["api_version"], json!(4), "{hello}");
    }
    // They are two Cores, not one reached twice: stopping the explicit one
    // leaves the default one serving.
    explicit.stop().await;
    assert_eq!(default.hello().await["api_version"], json!(4));
    default.stop().await;
}
