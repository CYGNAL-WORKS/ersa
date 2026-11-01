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
        "params": { "client": "ersa-cli-tests", "client_version": "0", "api_versions": [1] } })];
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

/// The Core playing the user's own: on Unix it serves its true default,
/// moved into the test's private directory through `XDG_RUNTIME_DIR`. A
/// Windows pipe name has no such override, so there it serves an endpoint
/// labelled for this test, which stands in for the default pipe this test
/// must not touch.
async fn start_default(dir: &Path) -> Core {
    if cfg!(unix) {
        let run = dir.join("run");
        std::fs::create_dir(&run).expect("run directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o700))
                .expect("private run directory");
        }
        let endpoint = run.join("ersa-core.sock").to_string_lossy().into_owned();
        let env = [("XDG_RUNTIME_DIR".into(), run.into_os_string())];
        start(dir, endpoint, &[], &env).await
    } else {
        let label = format!("cli-test-default-{}", std::process::id());
        let endpoint = ersa_core::platform::labelled_endpoint(&label).expect("endpoint");
        start(dir, endpoint, &["--endpoint", &label], &[]).await
    }
}

#[tokio::test]
async fn a_core_on_an_explicit_endpoint_runs_beside_the_default_one() {
    let first = tempfile::tempdir().expect("temp dir");
    let second = tempfile::tempdir().expect("temp dir");
    let default = start_default(first.path()).await;

    let label = format!("cli-test-{}", std::process::id());
    let endpoint = ersa_core::platform::labelled_endpoint(&label).expect("endpoint");
    assert_ne!(endpoint, default.endpoint);
    let explicit = start(
        second.path(),
        endpoint.clone(),
        &["--endpoint", &label],
        &[],
    )
    .await;

    // Both answer `core.hello`, each on its own endpoint.
    for core in [&default, &explicit] {
        let hello = core.hello().await;
        assert_eq!(hello["api_version"], json!(1), "{hello}");
    }
    // They are two Cores, not one reached twice: stopping the explicit one
    // leaves the default one serving.
    explicit.stop().await;
    assert_eq!(default.hello().await["api_version"], json!(1));
    default.stop().await;
}
