//! A Core started in-process on a private endpoint and data directory, and a
//! client that talks to it over the real socket.
//!
//! Every response the client reads is validated against the Core API schema
//! before a test sees it (PROTO-CORE-API-R06), so each test checks the schema
//! for every method it calls.

// Each test file compiles this module separately and uses a different part of it.
#![allow(dead_code)]

pub mod fake;
pub mod procs;
pub mod suite;

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use ersa_core::platform::{self, BoxStream};
use ersa_core::replay::Replay;
use ersa_core::{Adapter, CoreConfig, Server};
use ersa_protocol::schema::{validate_notification, validate_response};
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

/// A unique endpoint per test, labelled with the process id and a counter,
/// so parallel tests and a real Core never collide.
pub fn unique_endpoint() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    platform::labelled_endpoint(&format!(
        "test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
    .expect("endpoint")
}

pub fn test_config(data_dir: &Path, endpoint: &str) -> CoreConfig {
    let mut config = CoreConfig::new(data_dir.to_path_buf()).expect("config");
    config.endpoint = endpoint.to_string();
    config.backend_program_override = Some(NO_SUCH_BACKEND.to_string());
    config
}

/// Configures a test Core to detect backends by running their own version
/// commands, so a fake backend is found available and asked for its models.
pub fn detecting(config: &mut CoreConfig) {
    config.backend_program_override = None;
}

/// A new temporary directory, under the canonical form of the system's
/// temporary directory.
///
/// Core canonicalises the paths it stores, so tests compare against
/// canonical paths too. The system's own may not be: on GitHub's Windows
/// runners `TEMP` is an 8.3 short path (`C:\Users\RUNNER~1\...`), which Core
/// stores as `C:\Users\runneradmin\...`.
pub fn tempdir() -> tempfile::TempDir {
    let base = platform::canonicalize(&std::env::temp_dir()).expect("canonical temp dir");
    tempfile::tempdir_in(base).expect("temp dir")
}

/// Starts a Core in a temporary data directory.
pub async fn start() -> TestCore {
    start_with(tempdir(), |_| {}).await
}

/// Starts a Core whose data directory is `dir/data`, with `configure`
/// applied to its configuration.
pub async fn start_with(
    dir: tempfile::TempDir,
    configure: impl FnOnce(&mut CoreConfig),
) -> TestCore {
    start_with_adapters(dir, configure, Vec::new()).await
}

/// [`start_with`], with adapters registered.
pub async fn start_with_adapters(
    dir: tempfile::TempDir,
    configure: impl FnOnce(&mut CoreConfig),
    adapters: Vec<Arc<dyn Adapter>>,
) -> TestCore {
    let endpoint = unique_endpoint();
    let mut config = test_config(&dir.path().join("data"), &endpoint);
    configure(&mut config);
    let server = Server::bind_with(config, adapters).await.expect("bind");
    let task = tokio::spawn(server.run());
    TestCore {
        dir,
        endpoint,
        task,
    }
}

/// A replay server for `replay` in a temporary data directory, with
/// `configure` applied to its configuration.
pub async fn start_replay_of(replay: Replay, configure: impl FnOnce(&mut CoreConfig)) -> TestCore {
    let dir = tempdir();
    let endpoint = unique_endpoint();
    let mut config = test_config(&dir.path().join("data"), &endpoint);
    configure(&mut config);
    let server = Server::bind_replay(config, replay).await.expect("bind");
    let task = tokio::spawn(server.run());
    TestCore {
        dir,
        endpoint,
        task,
    }
}

/// The one session a replay server serves.
pub async fn the_session(client: &mut Client) -> Value {
    let sessions = client.result("session.list", json!({})).await;
    let sessions = sessions.as_array().expect("a list");
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    sessions[0].clone()
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

/// Waits until the `type`s of the app log's facts about session `id`, oldest
/// first, are exactly `expected`, and fails after about 10 s. Core hands
/// facts to the log's writer thread without waiting for the disk, so a test
/// polls the file, a bounded while, rather than reading it once and racing
/// the writer.
pub async fn app_log_facts(data_dir: &Path, id: &str, expected: &[&str]) {
    let mut seen = Vec::new();
    // The last read that failed, reported if the deadline passes. A read can
    // fail for a moment: on Windows a scanner holding the file without read
    // sharing gives os error 32, which is retried like a mismatch.
    let mut last_error = None;
    for _ in 0..100 {
        match ersa_core::app_log::read_entries(data_dir) {
            Ok(entries) => {
                seen = entries
                    .into_iter()
                    .filter(|e| e.session_id == id)
                    .map(|e| {
                        serde_json::to_value(&e.fact).unwrap()["type"]
                            .as_str()
                            .unwrap()
                            .to_string()
                    })
                    .collect::<Vec<String>>();
                if seen == expected {
                    return;
                }
            }
            Err(e) => last_error = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        seen, expected,
        "the app log's facts about {id}; last read error: {last_error:?}"
    );
}

pub struct Client {
    lines: Lines<BufReader<ReadHalf<BoxStream>>>,
    write: WriteHalf<BoxStream>,
    next_id: i64,
    /// Notifications that arrived while waiting for a response.
    notifications: VecDeque<Value>,
    /// Responses that arrived while waiting for another one, by id: Core
    /// may answer session requests out of order.
    responses: std::collections::HashMap<i64, Value>,
    /// The method of each request sent and not yet answered, by id.
    methods: std::collections::HashMap<i64, String>,
}

impl Client {
    pub async fn connect(endpoint: &str) -> Client {
        let stream = ersa_core::connect(endpoint).await.expect("connect");
        let (read, write) = tokio::io::split(stream);
        Client {
            lines: BufReader::new(read).lines(),
            write,
            next_id: 1,
            notifications: VecDeque::new(),
            responses: std::collections::HashMap::new(),
            methods: std::collections::HashMap::new(),
        }
    }

    pub async fn hello(&mut self) -> Value {
        self.result(
            "core.hello",
            json!({ "client": "ersa-core-tests", "client_version": "0", "api_versions": [5] }),
        )
        .await
    }

    /// Sends one line as is.
    pub async fn send_line(&mut self, line: &str) {
        self.send_bytes(format!("{line}\n").as_bytes()).await;
    }

    /// Sends raw bytes, which need not be a line or even text.
    pub async fn send_bytes(&mut self, bytes: &[u8]) {
        // Core may close the connection partway through a large write.
        if self.write.write_all(bytes).await.is_ok() {
            self.write.flush().await.ok();
        }
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

    /// The next line as text, unparsed; `None` once Core has closed the
    /// connection. For connections Core may cut off mid-line.
    pub async fn read_raw(&mut self) -> Option<String> {
        let line = tokio::time::timeout(READ_TIMEOUT, self.lines.next_line())
            .await
            .expect("Core answered in time");
        line.ok().flatten()
    }

    /// Sends a request and returns the whole response, after checking it
    /// against the schema and that it answers this request.
    pub async fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.request(method, params).await;
        self.response(id).await
    }

    /// Sends a request without waiting for its answer; returns its id.
    pub async fn request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.methods.insert(id, method.to_string());
        self.send_line(&request.to_string()).await;
        id
    }

    /// The response to request `id`, checked against the schema. Other
    /// responses and notifications that arrive first are kept.
    pub async fn response(&mut self, id: i64) -> Value {
        let response = loop {
            if let Some(response) = self.responses.remove(&id) {
                break response;
            }
            let line = self.read_line().await.expect("a response");
            match line.get("id").and_then(Value::as_i64) {
                Some(other) => {
                    self.responses.insert(other, line);
                }
                None if line.get("id").is_some() => panic!("a response without our id: {line:#}"),
                None => {
                    assert_valid_notification(&line);
                    self.notifications.push_back(line);
                }
            }
        };
        let method = self.methods.remove(&id).expect("a request we sent");
        assert_valid(&method, &response);
        response
    }

    /// How many responses arrived ahead of the one being waited for.
    pub fn responses_waiting(&self) -> usize {
        self.responses.len()
    }

    /// Forgets the notifications that arrived before the last response.
    pub fn clear_notifications(&mut self) {
        self.notifications.clear();
    }

    /// The next notification, kept from earlier or read now; `None` once Core
    /// has closed the connection. Checked against the schema.
    pub async fn notification(&mut self) -> Option<Value> {
        if let Some(notification) = self.notifications.pop_front() {
            return Some(notification);
        }
        let line = self.read_line().await?;
        assert!(line.get("id").is_none(), "a notification, not {line:#}");
        assert_valid_notification(&line);
        Some(line)
    }

    /// Reads notifications until one satisfies `wanted`, and returns it;
    /// those before it are dropped.
    pub async fn notification_where(&mut self, wanted: impl Fn(&Value) -> bool) -> Value {
        loop {
            let notification = self
                .notification()
                .await
                .expect("the connection stays open");
            if wanted(&notification) {
                return notification;
            }
        }
    }

    /// Reads `session.event` notifications, skipping others, until one whose
    /// event has type `event_type`, and returns every event read up to and
    /// including it.
    pub async fn events_until(&mut self, event_type: &str) -> Vec<Value> {
        let mut events = Vec::new();
        loop {
            let notification = self
                .notification()
                .await
                .expect("the connection stays open");
            if notification["method"] != "session.event" {
                continue;
            }
            let event = notification["params"]["event"].clone();
            let done = event["event"]["type"] == event_type;
            events.push(event);
            if done {
                return events;
            }
        }
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

/// Checks a notification against the schema (PROTO-CORE-API-R06).
pub fn assert_valid_notification(notification: &Value) {
    if let Err(e) = validate_notification(notification) {
        panic!(
            "notification does not match the schema:
{e}
{notification:#}"
        );
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
    git_repo_on(dir, "main");
}

/// A git repository at `dir` with one commit on `branch`.
pub fn git_repo_on(dir: &Path, branch: &str) {
    std::fs::create_dir_all(dir).expect("create repo dir");
    git(
        dir,
        &["init", "--quiet", &format!("--initial-branch={branch}")],
    );
    std::fs::write(dir.join("README.md"), "test repository\n").expect("write file");
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "--quiet", "-m", "initial"]);
}

/// The path as Core stores it: rebuilt from components, so separators are the
/// platform's own.
pub fn normalized(path: &Path) -> String {
    path.components().collect::<PathBuf>().display().to_string()
}

/// A git repository at `dir/repo` added as a project.
pub async fn project(client: &mut Client, dir: &Path) -> Value {
    let repo = dir.join("repo");
    git_repo(&repo);
    client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false }),
        )
        .await
}

/// A git repository at `dir/repo` added as a project, with one workspace;
/// returns the workspace's id.
pub async fn workspace(client: &mut Client, dir: &Path) -> String {
    let project = project(client, dir).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    workspace["id"].as_str().expect("workspace id").to_string()
}
