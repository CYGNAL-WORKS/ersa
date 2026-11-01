//! A Core started in-process on a private endpoint and data directory, and a
//! client that talks to it over the real socket.
//!
//! Every response the client reads is validated against the Core API schema
//! before a test sees it (PROTO-CORE-API-R06), so each test checks the schema
//! for every method it calls.

// Each test file compiles this module separately and uses a different part of it.
#![allow(dead_code)]

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use ersa_core::platform::{self, BoxStream};
use ersa_core::{CoreConfig, Server};
use ersa_protocol::schema::validate_response;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines, ReadHalf, WriteHalf};
use tokio::task::JoinHandle;

/// How long a test waits for any one line before failing instead of hanging.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// A program name that exists nowhere, so backend detection is fast and the
/// same on every machine.
pub const NO_SUCH_BACKEND: &str = "ersa-test-no-such-backend";

pub struct TestCore {
    pub dir: tempfile::TempDir,
    pub endpoint: String,
    pub task: JoinHandle<io::Result<()>>,
}

/// A unique endpoint per test: the platform default plus the process id and
/// a counter, so parallel tests and a real Core never collide.
pub fn unique_endpoint() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "{}-test-{}-{}",
        platform::default_endpoint(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn test_config(data_dir: &Path, endpoint: &str) -> CoreConfig {
    CoreConfig {
        data_dir: data_dir.to_path_buf(),
        endpoint: endpoint.to_string(),
        backend_program_override: Some(NO_SUCH_BACKEND.to_string()),
    }
}

/// Starts a Core in a temporary data directory.
pub async fn start() -> TestCore {
    let dir = tempfile::tempdir().expect("temp dir");
    let endpoint = unique_endpoint();
    let server = Server::bind(test_config(&dir.path().join("data"), &endpoint))
        .await
        .expect("bind");
    let task = tokio::spawn(server.run());
    TestCore {
        dir,
        endpoint,
        task,
    }
}

impl TestCore {
    /// A connected client that has already said hello.
    pub async fn client(&self) -> Client {
        let mut client = Client::connect(&self.endpoint).await;
        client.hello().await;
        client
    }

    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }
}

pub struct Client {
    lines: Lines<BufReader<ReadHalf<BoxStream>>>,
    write: WriteHalf<BoxStream>,
    next_id: i64,
}

impl Client {
    pub async fn connect(endpoint: &str) -> Client {
        let stream = ersa_core::connect(endpoint).await.expect("connect");
        let (read, write) = tokio::io::split(stream);
        Client {
            lines: BufReader::new(read).lines(),
            write,
            next_id: 1,
        }
    }

    pub async fn hello(&mut self) -> Value {
        self.result(
            "core.hello",
            json!({ "client": "ersa-core-tests", "client_version": "0", "api_versions": [1] }),
        )
        .await
    }

    /// Sends one line as is.
    pub async fn send_line(&mut self, line: &str) {
        self.write
            .write_all(format!("{line}\n").as_bytes())
            .await
            .expect("write");
        self.write.flush().await.expect("flush");
    }

    /// The next line from Core, or `None` once Core has closed the connection.
    pub async fn read_line(&mut self) -> Option<Value> {
        let line = tokio::time::timeout(READ_TIMEOUT, self.lines.next_line())
            .await
            .expect("Core answered in time");
        match line {
            Ok(Some(line)) => Some(serde_json::from_str(&line).expect("Core writes JSON")),
            Ok(None) => None,
            // A closed pipe can surface as an error rather than end-of-file.
            Err(_) => None,
        }
    }

    /// Sends a request and returns the whole response, after checking it
    /// against the schema and that it answers this request.
    pub async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send_line(&request.to_string()).await;
        let response = self.read_line().await.expect("a response");
        assert_valid(method, &response);
        assert_eq!(
            response["id"],
            json!(id),
            "answers the request it was sent for"
        );
        response
    }

    /// Calls and returns `result`, failing the test on an error.
    pub async fn result(&mut self, method: &str, params: Value) -> Value {
        let response = self.call(method, params).await;
        match response.get("result") {
            Some(result) => result.clone(),
            None => panic!("{method} failed: {response:#}"),
        }
    }

    /// Calls and returns `error`, failing the test on success.
    pub async fn error(&mut self, method: &str, params: Value) -> Value {
        let response = self.call(method, params).await;
        match response.get("error") {
            Some(error) => error.clone(),
            None => panic!("{method} unexpectedly succeeded: {response:#}"),
        }
    }
}

/// Checks a response against the schema (PROTO-CORE-API-R06).
pub fn assert_valid(method: &str, response: &Value) {
    if let Err(e) = validate_response(method, response) {
        panic!("{method} response does not match the schema:\n{e}\n{response:#}");
    }
}

/// `data.kind` of an application error.
pub fn kind(error: &Value) -> &str {
    error["data"]["kind"].as_str().unwrap_or("<none>")
}

// -- Git fixtures -------------------------------------------------------------

/// Runs git in `dir` and returns stdout; panics on failure. Tests may use
/// `std::process` directly: the rule that VCS commands go through the
/// transport is for Core.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        // Commits need an identity; these keep the test independent of the
        // user's git configuration.
        .args([
            "-c",
            "user.name=Ersa Tests",
            "-c",
            "user.email=tests@ersa.invalid",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A new git repository at `dir` with one commit on `main`.
pub fn git_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create repo dir");
    git(dir, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(dir.join("README.md"), "test repository\n").expect("write file");
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "--quiet", "-m", "initial"]);
}

/// The path as Core stores it: rebuilt from components, so separators are the
/// platform's own.
pub fn normalized(path: &Path) -> String {
    path.components().collect::<PathBuf>().display().to_string()
}
