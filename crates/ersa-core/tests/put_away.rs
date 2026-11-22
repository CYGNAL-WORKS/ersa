//! Putting an isolated workspace away (FLOW-CLEAN-UP, Putting away and
//! putting back), against real git repositories: reached through the
//! operation's own interface in Core, since no request asks for it yet, and
//! then through the Core API, method by method (PROTO-CORE-API-R123).

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CoreConfig, Gate};
use serde_json::{Value, json};
use support::deleting::{FailingGit, Removal, hold_the_turn, with_stopping_adds};
use support::fake::{self, FakeAdapter};
use support::{Client, TestCore, git, git_repo, kind, start_with_adapters, tempdir};

/// A Core with the fake backend, a git repository added as a project, and
/// `workspaces.root` in the test's folder.
struct Setup {
    core: TestCore,
    client: Client,
    repo: PathBuf,
    project: Value,
}

async fn setup() -> Setup {
    setup_with(|_| {}).await
}

async fn setup_with(configure: impl FnOnce(&mut CoreConfig)) -> Setup {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(tempdir(), configure, adapters).await;
    Setup::over(core).await
}

impl Setup {
    async fn over(core: TestCore) -> Setup {
        let mut client = core.client().await;
        let repo = core.dir.path().join("repo");
        git_repo(&repo);
        let root = core.dir.path().join("worktrees").display().to_string();
        let patch = json!({ "patch": { "workspaces": { "root": root } } });
        client.result("settings.set", patch).await;
        let add =
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false });
        let project = client.result("project.add", add).await;
        Setup {
            core,
            client,
            repo,
            project,
        }
    }

    async fn workspace(&mut self) -> Value {
        let params = json!({ "project_id": self.project["id"] });
        self.client.result("workspace.create", params).await
    }

    /// Puts `workspace` away through Core's own interface.
    async fn put_away(&self, workspace: &Value) -> Result<(), ersa_core::api::ApiError> {
        let core = self.core.core();
        ersa_core::api::put_away_workspace(&core, id(workspace)).await
    }

    /// `workspace` as `workspace.list` lists it now.
    async fn listed(&mut self, workspace: &Value) -> Value {
        let params = json!({ "project_id": self.project["id"] });
        let listed = self.client.result("workspace.list", params).await;
        listed
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["id"] == workspace["id"])
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// What Core stored of `workspace`'s checkout.
    async fn stored(&self, workspace: &Value) -> Option<ersa_core::workspace::Checkout> {
        let core = self.core.core();
        let id = id(workspace).to_string();
        core.store.put_away_checkout(id).await.expect("read")
    }

    /// A session of the fake backend in `workspace`, started, with a turn
    /// behind it, then stopped.
    async fn stopped_session(&mut self, workspace: &Value) -> String {
        let params = json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND });
        let session = self.client.result("session.create", params).await;
        let session = session["id"].as_str().unwrap().to_string();
        let subscribe = json!({ "id": session, "from_seq": 0 });
        self.client.result("session.subscribe", subscribe).await;
        self.client.events_until("session.started").await;
        // The fake backend finishes a turn whose message is `idle`.
        let send = json!({ "id": session, "text": "idle" });
        self.client.result("session.send", send).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let listed = self.client.result("session.list", json!({})).await;
            let state = listed
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["id"] == session)
                .unwrap()["state"]
                .clone();
            if state == "idle" {
                break;
            }
            assert!(
                tokio::time::Instant::now() < deadline,
                "never idle: {state}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.client
            .result("session.stop", json!({ "id": session }))
            .await;
        session
    }
}

fn id(workspace: &Value) -> &str {
    workspace["id"].as_str().unwrap()
}

fn folder(workspace: &Value) -> PathBuf {
    PathBuf::from(workspace["path"].as_str().unwrap())
}

fn branch(workspace: &Value) -> &str {
    workspace["vcs_ref"].as_str().unwrap()
}

fn has_branch(repo: &Path, branch: &str) -> bool {
    !git(repo, &["branch", "--list", branch]).trim().is_empty()
}

fn listed_by_git(repo: &Path, workspace: &Value) -> bool {
    git(repo, &["worktree", "list", "--porcelain"]).contains(id(workspace))
}

/// Commits a new file in `dir`, and returns the commit's id.
fn commit(dir: &Path, name: &str) -> String {
    std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "--quiet", "-m", name]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Lets every stop at `gate` go however the test ends, so no try at a
/// folder stays parked.
struct LetGo(Arc<Gate>);

impl Drop for LetGo {
    fn drop(&mut self) {
        for _ in 0..16 {
            self.0.go();
        }
    }
}

async fn reached(gate: &Gate) {
    tokio::time::timeout(Duration::from_secs(60), gate.reached())
        .await
        .expect("Core reached the gate in time");
}

// -- What putting away does (FLOW-CLEAN-UP-R02) ----------------------------

/// The folder and git's record of the worktree go; the workspace, its
/// session and every branch stay, with the branch the folder had checked
/// out stored, even one the agent switched to.
// verifies: FLOW-CLEAN-UP-R02
#[tokio::test(flavor = "multi_thread")]
async fn putting_away_removes_the_folder_and_keeps_the_workspace_its_sessions_and_branches() {
    let mut s = setup().await;
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;
    let path = folder(&workspace);
    // The agent switched to a branch of its own.
    git(&path, &["switch", "--quiet", "-c", "agents-own"]);
    commit(&path, "work.txt");

    s.put_away(&workspace).await.expect("put away");

    assert!(!path.exists(), "the folder is gone");
    assert!(!listed_by_git(&s.repo, &workspace), "git's record is gone");
    assert!(has_branch(&s.repo, branch(&workspace)), "its branch stays");
    assert!(has_branch(&s.repo, "agents-own"), "every branch stays");
    let listed = s.listed(&workspace).await;
    assert_eq!(listed["state"], "put_away", "{listed}");
    assert_eq!(listed["path"], workspace["path"], "where it comes back");
    let checkout = ersa_core::workspace::Checkout::Branch("agents-own".into());
    assert_eq!(s.stored(&workspace).await, Some(checkout));
    let sessions = s.client.result("session.list", json!({})).await;
    assert!(
        sessions
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == session),
        "its session stays: {sessions:#}"
    );
}

/// A folder on no branch stores the commit it is on.
// verifies: FLOW-CLEAN-UP-R02
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_on_no_branch_stores_its_commit() {
    let mut s = setup().await;
    let workspace = s.workspace().await;
    let path = folder(&workspace);
    git(&path, &["switch", "--quiet", "--detach"]);
    let detached = commit(&path, "detached.txt");

    s.put_away(&workspace).await.expect("put away");

    assert!(!path.exists());
    let checkout = ersa_core::workspace::Checkout::Commit(detached);
    assert_eq!(s.stored(&workspace).await, Some(checkout));
    assert!(has_branch(&s.repo, branch(&workspace)));
}

/// The state and the checkout are stored before anything is removed: with
/// git's removal stopped, the workspace already reads `put_away`, its
/// folder still there.
// verifies: FLOW-CLEAN-UP-R02
#[tokio::test(flavor = "multi_thread")]
async fn the_workspace_is_put_away_before_anything_is_removed() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let git_stops =
        Arc::new(FailingGit::new(Removal::Fails, None).stopping_removals(Arc::clone(&gate)));
    let mut s = setup_with(|config| {
        config.command_transport = Some(ersa_core::CommandTransport(git_stops));
    })
    .await;
    let workspace = s.workspace().await;
    let core = s.core.core();
    let putting = tokio::spawn({
        let id = id(&workspace).to_string();
        async move { ersa_core::api::put_away_workspace(&core, &id).await }
    });

    reached(&gate).await;
    assert!(folder(&workspace).exists());
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
    let checkout = ersa_core::workspace::Checkout::Branch(branch(&workspace).into());
    assert_eq!(s.stored(&workspace).await, Some(checkout));

    gate.go();
    // git then fails with the worktree still listed: it is left put away,
    // for the next put-away to finish.
    let answer = putting.await.unwrap();
    assert!(answer.is_err(), "{answer:?}");
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
}

/// A put-away refused before anything was removed leaves the workspace
/// `ready`, with nothing stored: a folder with changes not committed.
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_with_changes_is_not_put_away() {
    let mut s = setup().await;
    let workspace = s.workspace().await;
    let path = folder(&workspace);
    std::fs::write(path.join("unsaved.txt"), "unsaved\n").unwrap();

    let refused = s.put_away(&workspace).await;

    assert!(refused.is_err());
    assert!(path.join("unsaved.txt").exists());
    assert_eq!(s.listed(&workspace).await["state"], "ready");
    assert_eq!(s.stored(&workspace).await, None);
}

// -- As every removal does (PROTO-CORE-API-R122, R124, R131) ---------------

/// git's removal fails after it dropped its record of the worktree: the
/// put-away goes on to the folder itself, as every removal does, and keeps
/// the branch, which every other removal deletes.
// verifies: PROTO-CORE-API-R122
#[tokio::test(flavor = "multi_thread")]
async fn putting_away_goes_on_to_the_folder_when_git_fails_after_dropping_its_record() {
    let git_drops = Arc::new(FailingGit::new(Removal::DropsRecordThenFails, None));
    let mut s = setup_with(|config| {
        config.command_transport = Some(ersa_core::CommandTransport(git_drops));
    })
    .await;
    let workspace = s.workspace().await;

    s.put_away(&workspace).await.expect("put away");

    assert!(!folder(&workspace).exists(), "the folder is gone");
    assert!(!listed_by_git(&s.repo, &workspace));
    assert!(has_branch(&s.repo, branch(&workspace)), "the branch stays");
    assert_eq!(s.listed(&workspace).await["state"], "put_away");
}

/// A put-away waits its turn at the project's worktrees; past the wait it
/// starts none of its git work, and the workspace is as it was.
// verifies: PROTO-CORE-API-R124
#[tokio::test(flavor = "multi_thread")]
async fn a_put_away_that_waited_too_long_for_its_turn_removes_nothing() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let (core, git_calls, mut client, project) =
        with_stopping_adds(&gate, Duration::from_millis(300), adapters).await;
    let workspace = support::deleting::made_through(&gate, &mut client, &project).await;
    let holding = hold_the_turn(&gate, &mut client, &project).await;
    let before = git_calls.changes().len();

    let answer = ersa_core::api::put_away_workspace(&core.core(), id(&workspace)).await;

    let error = answer.expect_err("no turn in time");
    assert_eq!(error_kind(&error), "vcs_error", "{error:?}");
    assert_eq!(git_calls.changes().len(), before, "no git work began");
    assert!(folder(&workspace).exists());
    let listed = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    let listed = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == workspace["id"]);
    assert_eq!(listed.unwrap()["state"], "ready");
    gate.go();
    client.response(holding).await;
}

/// The error kind of `error`, as the wire names it.
fn error_kind(error: &ersa_core::api::ApiError) -> Value {
    let data = error.0.data.as_ref().expect("an application error");
    serde_json::to_value(data.kind).unwrap()
}

/// A Core whose git drops its record of a worktree and then fails, so a
/// removal goes on to the folder, and whose tries at a folder stop at
/// `gate` and give up after 200 ms.
async fn stuck_folder_setup(gate: &Arc<Gate>) -> Setup {
    let git_drops = Arc::new(FailingGit::new(Removal::DropsRecordThenFails, None));
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let configure = {
        let gate = Arc::clone(gate);
        move |config: &mut CoreConfig| {
            config.command_transport = Some(ersa_core::CommandTransport(git_drops));
            config.folder_gate = Some(gate);
            config.folder_removal_limit = Duration::from_millis(200);
        }
    };
    let core = start_with_adapters(tempdir(), configure, adapters).await;
    let mut client = core.client().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let root = core.dir.path().join("worktrees").display().to_string();
    let patch = json!({ "patch": { "workspaces": { "root": root } } });
    client.result("settings.set", patch).await;
    // The project's folder is inspected at the gate too.
    let add = json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false });
    let adding = client.request("project.add", add).await;
    reached(gate).await;
    gate.go();
    let project = client.response(adding).await["result"].clone();
    Setup {
        core,
        client,
        repo,
        project,
    }
}

/// While a try at the folder that the put-away gave up on has not ended,
/// no other removal of the folder or of the workspace begins: a
/// `workspace.remove` and a `session.delete` answer `busy`, and a second
/// put-away is refused. Once the try ends, the workspace can be removed.
// verifies once #560 lands: PROTO-CORE-API-R131
#[tokio::test(flavor = "multi_thread")]
async fn a_put_away_holds_the_workspace_while_a_try_it_gave_up_on_is_stuck() {
    let gate = Arc::new(Gate::default());
    let _let_go = LetGo(Arc::clone(&gate));
    let mut s = stuck_folder_setup(&gate).await;
    let workspace = s.workspace().await;
    let session = s.stopped_session(&workspace).await;

    let putting = tokio::spawn({
        let core = s.core.core();
        let id = id(&workspace).to_string();
        async move { ersa_core::api::put_away_workspace(&core, &id).await }
    });
    reached(&gate).await;
    assert!(putting.await.unwrap().is_err(), "the try ran out");

    for _ in 0..2 {
        let error = s
            .client
            .error(
                "workspace.remove",
                json!({ "id": workspace["id"], "force": true }),
            )
            .await;
        assert_eq!(kind(&error), "invalid_state", "{error}");
        let token = s
            .client
            .result("workspace.changes", json!({ "id": workspace["id"] }))
            .await["token"]
            .clone();
        let error = s
            .client
            .error("session.delete", json!({ "id": session, "token": token }))
            .await;
        assert_eq!(kind(&error), "invalid_state", "{error}");
        assert!(
            error["message"].as_str().unwrap().contains("busy"),
            "{error}"
        );
        assert!(s.put_away(&workspace).await.is_err(), "no second put-away");
    }
    assert!(folder(&workspace).exists(), "nothing else removed it");
    assert_eq!(s.listed(&workspace).await["state"], "put_away");

    // The stuck try returns; then the workspace is removed.
    gate.go();
    let started = std::time::Instant::now();
    loop {
        let params = json!({ "id": session });
        let _ = s.client.call("session.remove", params).await;
        let remove = json!({ "id": workspace["id"], "force": true });
        let answer = s.client.call("workspace.remove", remove).await;
        if answer.get("result").is_some() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the hold was never let go: {answer}"
        );
        // A try at the folder stops at the gate again: let it through.
        gate.go();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
