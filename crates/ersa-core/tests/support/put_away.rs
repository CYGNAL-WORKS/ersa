//! A Core with the fake backend and a git repository added as a project,
//! for putting workspaces away and back (FLOW-CLEAN-UP).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ersa_core::{Adapter, CoreConfig, Gate};
use serde_json::{Value, json};

use super::deleting::{FailingGit, Removal};
use super::fake::{self, FakeAdapter};
use super::{Client, TestCore, git, git_repo, start_with_adapters, tempdir};

/// A Core with the fake backend, a git repository added as a project, and
/// `workspaces.root` in the test's folder.
pub struct Setup {
    pub core: TestCore,
    pub client: Client,
    pub repo: PathBuf,
    pub project: Value,
    /// What the fake backend was asked to do.
    pub stats: Arc<fake::Stats>,
}

pub async fn setup() -> Setup {
    setup_with(|_| {}).await
}

pub async fn setup_with(configure: impl FnOnce(&mut CoreConfig)) -> Setup {
    let fake = FakeAdapter::new(1);
    let stats = Arc::clone(&fake.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(fake)];
    let core = start_with_adapters(tempdir(), configure, adapters).await;
    Setup::over(core, stats).await
}

/// A Core whose git drops its record of a worktree and then fails, so a
/// removal goes on to the folder, and whose tries at a folder stop at
/// `gate` and give up after 200 ms; with `wait` for each wait at the
/// project's worktrees.
pub async fn stuck_folder_setup(gate: &Arc<Gate>, wait: Duration) -> Setup {
    stuck_folder_setup_with(gate, wait, |_| {}).await
}

/// [`stuck_folder_setup`], with `more` applied to the configuration too.
pub async fn stuck_folder_setup_with(
    gate: &Arc<Gate>,
    wait: Duration,
    more: impl FnOnce(&mut CoreConfig),
) -> Setup {
    let git_drops = Arc::new(FailingGit::new(Removal::DropsRecordThenFails, None));
    let fake = FakeAdapter::new(1);
    let stats = Arc::clone(&fake.stats);
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(fake)];
    let configure = {
        let gate = Arc::clone(gate);
        move |config: &mut CoreConfig| {
            config.command_transport = Some(ersa_core::CommandTransport(git_drops));
            config.folder_gate = Some(gate);
            config.folder_removal_limit = Duration::from_millis(200);
            config.worktree_turn_wait = wait;
            more(config);
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
        stats,
    }
}

impl Setup {
    pub async fn over(core: TestCore, stats: Arc<fake::Stats>) -> Setup {
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
            stats,
        }
    }

    pub async fn workspace(&mut self) -> Value {
        let params = json!({ "project_id": self.project["id"] });
        self.client.result("workspace.create", params).await
    }

    /// Puts `workspace` away through Core's own interface.
    pub async fn put_away(&self, workspace: &Value) -> Result<(), ersa_core::api::ApiError> {
        let core = self.core.core();
        ersa_core::api::put_away_workspace(&core, id(workspace)).await
    }

    /// `workspace` as `workspace.list` lists it now.
    pub async fn listed(&mut self, workspace: &Value) -> Value {
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
    pub async fn stored(&self, workspace: &Value) -> Option<ersa_core::workspace::Checkout> {
        let core = self.core.core();
        let id = id(workspace).to_string();
        core.store.put_away_checkout(id).await.expect("read")
    }

    /// A session of the fake backend in `workspace`, started, with a turn
    /// behind it, then stopped.
    pub async fn stopped_session(&mut self, workspace: &Value) -> String {
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

impl Setup {
    /// Adds a submodule at `sub` to the project's repository, committed on
    /// its branch, so each workspace made from now on has it, not checked
    /// out. Its repository is beside the project's, with one file.
    pub fn add_submodule(&self) -> PathBuf {
        let source = self.core.dir.path().join("sub-source");
        std::fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--quiet", "--initial-branch=main"]);
        commit(&source, "sub.txt");
        let url = source.display().to_string();
        git(
            &self.repo,
            &["-c", "protocol.file.allow=always", "submodule", "add", "--quiet", &url, "sub"],
        );
        git(&self.repo, &["commit", "--quiet", "-m", "add a submodule"]);
        source
    }
}

/// Checks out every submodule in the worktree at `dir`, as a person or an
/// agent would there.
pub fn check_out_submodules(dir: &Path) {
    git(
        dir,
        &["-c", "protocol.file.allow=always", "submodule", "update", "--init", "--quiet"],
    );
}

/// A repository of its own at `dir`, with one commit and nothing
/// uncommitted.
pub fn nested_repository(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet", "--initial-branch=main"]);
    commit(dir, "nested.txt");
}

pub fn id(workspace: &Value) -> &str {
    workspace["id"].as_str().unwrap()
}

pub fn folder(workspace: &Value) -> PathBuf {
    PathBuf::from(workspace["path"].as_str().unwrap())
}

pub fn branch(workspace: &Value) -> &str {
    workspace["vcs_ref"].as_str().unwrap()
}

pub fn has_branch(repo: &Path, branch: &str) -> bool {
    !git(repo, &["branch", "--list", branch]).trim().is_empty()
}

pub fn listed_by_git(repo: &Path, workspace: &Value) -> bool {
    git(repo, &["worktree", "list", "--porcelain"]).contains(id(workspace))
}

/// Commits a new file in `dir`, and returns the commit's id.
pub fn commit(dir: &Path, name: &str) -> String {
    std::fs::write(dir.join(name), format!("{name}\n")).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "--quiet", "-m", name]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Admission has `workspace` ready too, as the store does: a session can
/// be made there again.
pub fn admitted_ready(core: &ersa_core::Core, workspace: &Value) {
    let state = core.admission.snapshot();
    let phase = state.workspace(id(workspace)).map(|w| w.phase);
    assert_eq!(phase, Some(ersa_core::admission::WorkspacePhase::Ready));
}

/// Lets every stop at `gate` go however the test ends, so no try at a
/// folder stays parked.
pub struct LetGo(pub Arc<Gate>);

impl Drop for LetGo {
    fn drop(&mut self) {
        for _ in 0..16 {
            self.0.go();
        }
    }
}

pub async fn reached(gate: &Gate) {
    tokio::time::timeout(Duration::from_secs(60), gate.reached())
        .await
        .expect("Core reached the gate in time");
}

/// Waits, up to a generous limit, until nothing holds `workspace` and
/// admission has it put away: a try at its folder given up on has ended.
pub async fn put_away_once_held(core: &ersa_core::Core, workspace: &Value) {
    let phase = || {
        let state = core.admission.snapshot();
        state.workspace(id(workspace)).map(|w| w.phase)
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while phase() != Some(ersa_core::admission::WorkspacePhase::PutAway) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the hold was never let go: {:?}",
            phase()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
