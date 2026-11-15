//! Requests that run git or a backend's command, or inspect a folder the
//! user named, never hold up the connection's other requests
//! (PROTO-CORE-API-R92), and a folder that does not answer in time is
//! refused (PROTO-CORE-API-R93). Driven over the socket, with the slow part
//! held at a point the test controls, so every wait is on state, never on a
//! fixed pause.

mod support;

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ersa_adapter::{Process, ProcessSpec, Transport};
use ersa_core::transport::LocalTreeTransport;
use ersa_core::{Adapter, CoreConfig, Gate};
use serde_json::{Value, json};
use support::fake::FakeAdapter;
use support::{Client, TestCore, git_repo, kind, start_with_adapters, tempdir};
use tokio::sync::Semaphore;

/// Holds the next process Core starts, once armed, until the test lets it
/// go: a git call or a backend's version command that does not answer.
struct Stall {
    armed: AtomicBool,
    reached: Semaphore,
    go: Semaphore,
}

impl Stall {
    fn new() -> Arc<Stall> {
        Arc::new(Stall {
            armed: AtomicBool::new(false),
            reached: Semaphore::new(0),
            go: Semaphore::new(0),
        })
    }

    /// The next process started waits until [`Stall::go`].
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Waits until a process is held.
    async fn reached(&self) {
        within(self.reached.acquire()).await.expect("open").forget();
    }

    fn go(&self) {
        self.go.add_permits(1);
    }
}

/// How long a test waits for Core to reach a stop before failing instead
/// of hanging: a deadline, never a pause.
const DEADLINE: Duration = Duration::from_secs(60);

async fn within<T>(waited: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(DEADLINE, waited)
        .await
        .expect("Core reached the stop in time")
}

/// Waits until a request stops at `gate`.
async fn reached(gate: &Gate) {
    within(gate.reached()).await;
}

/// Lets every stop go when the test ends, passed or failed: an inspection
/// left parked at the gate on its own thread would keep the test's runtime
/// from shutting down, so a failing test would hang instead of failing.
struct LetGo(Arc<Stall>, Arc<Gate>);

impl Drop for LetGo {
    fn drop(&mut self) {
        // More than any test parks; a spare permit is never used.
        for _ in 0..8 {
            self.0.go();
            self.1.go();
        }
    }
}

/// A [`Stall`] and a closed [`Gate`], let go when the [`LetGo`] drops.
fn stops() -> (Arc<Stall>, Arc<Gate>, LetGo) {
    let (stall, gate) = (Stall::new(), Arc::new(Gate::default()));
    let let_go = LetGo(Arc::clone(&stall), Arc::clone(&gate));
    (stall, gate, let_go)
}

/// Core's command transport for the test: the local one, behind a [`Stall`].
struct StallingTransport {
    stall: Arc<Stall>,
    inner: LocalTreeTransport,
}

#[ersa_adapter::async_trait]
impl Transport for StallingTransport {
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        if self.stall.armed.swap(false, Ordering::SeqCst) {
            self.stall.reached.add_permits(1);
            self.stall.go.acquire().await.expect("open").forget();
        }
        self.inner.spawn(spec).await
    }
}

/// A Core with the fake backend registered (so detection runs a version
/// command), its commands run through `stall`, its folder inspections
/// stopping at `gate`, and `configure` applied after.
async fn start(
    stall: &Arc<Stall>,
    gate: &Arc<Gate>,
    configure: impl FnOnce(&mut CoreConfig),
) -> (TestCore, Client) {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let transport = Arc::new(StallingTransport {
        stall: Arc::clone(stall),
        inner: LocalTreeTransport::for_commands(),
    });
    let gate = Arc::clone(gate);
    let core = start_with_adapters(
        tempdir(),
        move |config| {
            config.command_transport = Some(transport);
            config.inspect_gate = Some(gate);
            configure(config);
        },
        adapters,
    )
    .await;
    let client = core.client().await;
    (core, client)
}

/// While the request `slow` is held, a `session.interrupt` and a
/// `session.list` sent after it on the same connection are answered.
async fn others_answer_while_held(client: &mut Client) {
    let interrupt = client
        .request("session.interrupt", json!({ "id": "s-none" }))
        .await;
    let list = client.request("session.list", json!({})).await;
    let interrupted = client.response(interrupt).await;
    assert_eq!(kind(&interrupted["error"]), "not_found", "{interrupted}");
    let listed = client.response(list).await;
    assert_eq!(listed["result"], json!([]), "{listed}");
}

fn add_params(path: &Path) -> Value {
    json!({ "host_id": "local", "path": path.display().to_string(), "trusted": false })
}

/// The params of `method`, `project.add` or `project.find`, for `path`.
fn folder_params(method: &str, path: &Path) -> Value {
    match method {
        "project.add" => add_params(path),
        _ => json!({ "host_id": "local", "path": path.display().to_string() }),
    }
}

/// Adds a git repository at `dir/repo` as a project, letting its inspection
/// through the gate; returns the project.
async fn added(client: &mut Client, gate: &Gate, dir: &Path) -> Value {
    let repo = dir.join("repo");
    git_repo(&repo);
    let add = client.request("project.add", add_params(&repo)).await;
    reached(gate).await;
    gate.go();
    client.response(add).await["result"].clone()
}

/// The inspection's limit for tests that hold it and must not run out.
const LONG: Duration = Duration::from_secs(600);

/// A request whose process Core holds: the others answer meanwhile, and
/// it answers once let go.
async fn held_process_holds_nothing_up(method: &str, params: impl FnOnce(&Value) -> Value) {
    let (stall, gate, _let_go) = stops();
    let (core, mut client) = start(&stall, &gate, |c| c.folder_inspection_limit = LONG).await;
    let project = added(&mut client, &gate, core.dir.path()).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let params = params(&json!({ "project": project, "workspace": workspace }));

    stall.arm();
    let slow = client.request(method, params).await;
    stall.reached().await;
    others_answer_while_held(&mut client).await;
    stall.go();
    let answer = client.response(slow).await;
    assert!(answer.get("result").is_some(), "{method}: {answer}");
}

// verifies: PROTO-CORE-API-R92
#[tokio::test(flavor = "multi_thread")]
async fn branch_list_holds_nothing_up() {
    held_process_holds_nothing_up(
        "branch.list",
        |made| json!({ "project_id": made["project"]["id"] }),
    )
    .await;
}

// verifies: PROTO-CORE-API-R92
#[tokio::test(flavor = "multi_thread")]
async fn workspace_create_holds_nothing_up() {
    held_process_holds_nothing_up(
        "workspace.create",
        |made| json!({ "project_id": made["project"]["id"] }),
    )
    .await;
}

// verifies: PROTO-CORE-API-R92
#[tokio::test(flavor = "multi_thread")]
async fn workspace_remove_holds_nothing_up() {
    held_process_holds_nothing_up(
        "workspace.remove",
        |made| json!({ "id": made["workspace"]["id"] }),
    )
    .await;
}

// verifies: PROTO-CORE-API-R92
#[tokio::test(flavor = "multi_thread")]
async fn backend_list_holds_nothing_up() {
    held_process_holds_nothing_up("backend.list", |_| json!({ "host_id": "local" })).await;
}

// verifies: PROTO-CORE-API-R92
#[tokio::test(flavor = "multi_thread")]
async fn host_list_holds_nothing_up() {
    held_process_holds_nothing_up("host.list", |_| json!({})).await;
}

/// A request whose folder inspection is held: the others answer meanwhile.
async fn held_inspection_holds_nothing_up(method: &str) {
    let (stall, gate, _let_go) = stops();
    let (core, mut client) = start(&stall, &gate, |c| c.folder_inspection_limit = LONG).await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);

    let slow = client.request(method, folder_params(method, &repo)).await;
    reached(&gate).await;
    others_answer_while_held(&mut client).await;
    gate.go();
    let answer = client.response(slow).await;
    assert!(answer.get("result").is_some(), "{method}: {answer}");
}

// verifies: PROTO-CORE-API-R92
#[tokio::test(flavor = "multi_thread")]
async fn project_add_holds_nothing_up() {
    held_inspection_holds_nothing_up("project.add").await;
}

// verifies: PROTO-CORE-API-R92
#[tokio::test(flavor = "multi_thread")]
async fn project_find_holds_nothing_up() {
    held_inspection_holds_nothing_up("project.find").await;
}

/// The limit in the tests that let it run out: short, so the test is quick,
/// with the inspection held well past it.
const SHORT: Duration = Duration::from_millis(200);

/// `project.add` of a folder that does not answer in time is refused with
/// `permission`, and the project is not added when the inspection later
/// returns: the same folder is then added as new.
// verifies: PROTO-CORE-API-R93
#[tokio::test(flavor = "multi_thread")]
async fn project_add_of_a_folder_that_does_not_answer_adds_nothing() {
    let (stall, gate, _let_go) = stops();
    let (core, mut client) = start(&stall, &gate, |c| c.folder_inspection_limit = SHORT).await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);

    let slow = client.request("project.add", add_params(&repo)).await;
    reached(&gate).await;
    let answer = client.response(slow).await;
    let error = &answer["error"];
    assert_eq!(kind(error), "permission", "{answer}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("did not answer in time"),
        "{answer}"
    );
    // The inspection returns now, to nobody.
    gate.go();
    assert_eq!(client.result("project.list", json!({})).await, json!([]));

    // Nothing was added: the same folder is new, once it answers in time.
    let retry = client.request("project.add", add_params(&repo)).await;
    reached(&gate).await;
    gate.go();
    let added = client.response(retry).await;
    assert_eq!(added["result"]["name"], json!("repo"), "{added}");
    assert_eq!(
        client.result("project.list", json!({})).await,
        json!([added["result"]])
    );
}

/// `project.find` of a folder that does not answer in time is refused with
/// `permission`, not answered null, so the folder of a project on a slow
/// share is not taken for a new one.
// verifies: PROTO-CORE-API-R93
#[tokio::test(flavor = "multi_thread")]
async fn project_find_of_a_folder_that_does_not_answer_is_refused_not_null() {
    let (stall, gate, _let_go) = stops();
    let (core, mut client) = start(&stall, &gate, |c| c.folder_inspection_limit = SHORT).await;

    let slow = client
        .request(
            "project.find",
            folder_params("project.find", &core.dir.path().join("gone")),
        )
        .await;
    reached(&gate).await;
    let answer = client.response(slow).await;
    let error = &answer["error"];
    assert_eq!(kind(error), "permission", "{answer}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("did not answer in time"),
        "{answer}"
    );
    gate.go();
}

/// Two `project.add` for one folder under way at once: one adds it, the
/// other is refused as for a folder already a project, naming it.
// verifies: PROTO-CORE-API-R20
#[tokio::test(flavor = "multi_thread")]
async fn two_adds_of_one_folder_at_once_add_it_once() {
    let (stall, gate, _let_go) = stops();
    let (core, mut client) = start(&stall, &gate, |c| c.folder_inspection_limit = LONG).await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);

    let first = client.request("project.add", add_params(&repo)).await;
    let second = client.request("project.add", add_params(&repo)).await;
    // Both have inspected nothing yet; then both go on together.
    reached(&gate).await;
    reached(&gate).await;
    gate.go();
    gate.go();
    let answers = [client.response(first).await, client.response(second).await];

    let added: Vec<&Value> = answers.iter().filter_map(|a| a.get("result")).collect();
    assert_eq!(added.len(), 1, "{answers:?}");
    let refused = answers
        .iter()
        .find_map(|a| a.get("error"))
        .expect("one refused");
    assert_eq!(kind(refused), "invalid_state", "{answers:?}");
    assert_eq!(refused["data"]["detail"], added[0]["id"], "{answers:?}");
    assert_eq!(
        client.result("project.list", json!({})).await,
        json!([added[0]])
    );
}

/// Core gives the inspection 10 seconds unless a test says otherwise; the
/// tests above use a short limit so they run quickly.
// verifies: PROTO-CORE-API-R93
#[test]
fn the_inspection_has_ten_seconds() {
    let config = CoreConfig::new(tempdir().path().to_path_buf()).expect("config");
    assert_eq!(config.folder_inspection_limit, Duration::from_secs(10));
}
