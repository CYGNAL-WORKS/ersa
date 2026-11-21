//! What `session.delete`'s tests share: a project, a workspace and an idle
//! session to delete, and a git that fails worktree removals.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ersa_core::Gate;
use ersa_core::transport::LocalTreeTransport;
use serde_json::{Value, json};

use super::fake;
use super::{Client, git_repo};

/// A git repository at `dir/repo`, added as a project; returns the project
/// and the repository's folder.
pub async fn project(client: &mut Client, dir: &Path) -> (Value, PathBuf) {
    let repo = dir.join("repo");
    git_repo(&repo);
    let project = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false }),
        )
        .await;
    (project, repo)
}

/// A new isolated workspace of `project`; returns it and its folder.
pub async fn isolated(client: &mut Client, project: &Value) -> (Value, PathBuf) {
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let folder = PathBuf::from(workspace["path"].as_str().unwrap());
    (workspace, folder)
}

/// The project's root workspace.
pub async fn root(client: &mut Client, project: &Value) -> Value {
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["kind"] == "root")
        .cloned()
        .expect("a root workspace")
}

/// A session of the fake backend in `workspace`, started and idle: its
/// backend has named it, so it has a record. Returns its id.
pub async fn idle_session(client: &mut Client, workspace: &Value) -> String {
    let session = client
        .result(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;
    let id = session["id"].as_str().unwrap().to_string();
    client
        .result("session.subscribe", json!({ "id": id, "from_seq": 0 }))
        .await;
    client.events_until("session.started").await;
    client
        .result("session.send", json!({ "id": id, "text": "idle" }))
        .await;
    wait_for_state(client, &id, "idle").await;
    id
}

/// Waits until `session.list` reports the session in `state`.
pub async fn wait_for_state(client: &mut Client, id: &str, state: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if listed(client, id)
            .await
            .is_some_and(|row| row["state"] == state)
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "session {id} never became {state}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The session's row in `session.list`, if it is listed.
pub async fn listed(client: &mut Client, id: &str) -> Option<Value> {
    let sessions = client.result("session.list", json!({})).await;
    sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
}

/// The token of the workspace's `workspace.changes` now.
pub async fn token(client: &mut Client, workspace: &Value) -> Value {
    client
        .result("workspace.changes", json!({ "id": workspace["id"] }))
        .await["token"]
        .clone()
}

/// What git does to a worktree removal in [`FailingGit`].
#[derive(Clone, Copy)]
pub enum Removal {
    /// Deletes git's record of the worktree and then fails, leaving the
    /// files, as git on Windows does on a file in use.
    DropsRecordThenFails,
    /// Fails before touching anything, so git still lists the worktree.
    Fails,
}

/// Core's command transport: the local one, with every
/// `git worktree remove` failing as `removal` says.
pub struct FailingGit {
    removal: Removal,
    /// When set, the first removal also holds the worktree's `in-use.txt`
    /// in use (Windows), as a process that has just exited does, for this
    /// long or until this stand-in is dropped: counted from git's first
    /// try, so the tries at the folder are what it outlasts.
    hold: Option<Duration>,
    /// Lets the held file go when dropped.
    held: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
    /// When set, each worktree's first removal stops here until the test
    /// lets it go.
    stop: Option<Arc<Gate>>,
    /// The worktrees whose first removal has been.
    stopped: std::sync::Mutex<std::collections::HashSet<String>>,
    inner: LocalTreeTransport,
}

impl FailingGit {
    /// Fails removals as `removal` says, holding a file in use for `hold`
    /// when set.
    pub fn new(removal: Removal, hold: Option<Duration>) -> FailingGit {
        FailingGit {
            removal,
            hold,
            held: std::sync::Mutex::new(None),
            stop: None,
            stopped: std::sync::Mutex::new(std::collections::HashSet::new()),
            inner: LocalTreeTransport::for_commands(),
        }
    }

    /// Each worktree's first removal also stops at `gate`, before git
    /// runs, until the test lets it go: past the delete's step 3.
    pub fn stopping_removals(mut self, gate: Arc<Gate>) -> FailingGit {
        self.stop = Some(gate);
        self
    }
}

#[ersa_adapter::async_trait]
impl ersa_adapter::Transport for FailingGit {
    async fn spawn(
        &self,
        mut spec: ersa_adapter::ProcessSpec,
    ) -> std::io::Result<Box<dyn ersa_adapter::Process>> {
        let removes = spec
            .args
            .windows(2)
            .any(|pair| pair == ["worktree", "remove"]);
        if spec.program == "git" && removes {
            let first = self
                .stopped
                .lock()
                .unwrap()
                .insert(spec.args.last().unwrap().clone());
            if let (Some(gate), true) = (&self.stop, first) {
                gate.stop().await;
            }
            match self.removal {
                Removal::DropsRecordThenFails => {
                    // The worktree's `.git` file names git's record of it.
                    // Gone after the first try; git's own removal then
                    // fails on every try: not a working tree.
                    let worktree = PathBuf::from(spec.args.last().unwrap());
                    let mut held = self.held.lock().unwrap();
                    if let (Some(hold), None) = (self.hold, held.as_ref()) {
                        let (release, released) = std::sync::mpsc::channel();
                        held_in_use(&worktree.join("in-use.txt"), hold, released);
                        *held = Some(release);
                    }
                    if let Ok(link) = std::fs::read_to_string(worktree.join(".git")) {
                        let record = link.trim().trim_start_matches("gitdir:").trim();
                        let _ = std::fs::remove_dir_all(record);
                    }
                }
                Removal::Fails => spec.args.push("--no-such-option".into()),
            }
        }
        self.inner.spawn(spec).await
    }
}

/// Holds `file` open for `held`, or until `released` hears or is
/// disconnected, sharing reading only, so deleting it fails on Windows
/// until then, as a process that has just exited holds files there.
/// Elsewhere an open file does not stop its deletion.
pub fn held_in_use(file: &Path, held: Duration, released: std::sync::mpsc::Receiver<()>) {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ alone: no delete sharing.
        options.share_mode(1);
    }
    let open = options.open(file).unwrap();
    std::thread::spawn(move || {
        let _ = released.recv_timeout(held);
        drop(open);
    });
}

/// Core's command transport, with every `git worktree add` stopping at
/// `gate` before git runs, until the test lets it go. It notes each change
/// it is given to a repository's worktrees: an add, a removal or a prune.
pub struct StoppingAdds {
    gate: Arc<Gate>,
    changes: std::sync::Mutex<Vec<String>>,
    inner: LocalTreeTransport,
}

impl StoppingAdds {
    pub fn new(gate: &Arc<Gate>) -> StoppingAdds {
        StoppingAdds {
            gate: Arc::clone(gate),
            changes: std::sync::Mutex::new(Vec::new()),
            inner: LocalTreeTransport::for_commands(),
        }
    }

    /// The changes to worktrees given so far: `add`, `remove` or `prune`.
    pub fn changes(&self) -> Vec<String> {
        self.changes.lock().unwrap().clone()
    }
}

#[ersa_adapter::async_trait]
impl ersa_adapter::Transport for StoppingAdds {
    async fn spawn(
        &self,
        spec: ersa_adapter::ProcessSpec,
    ) -> std::io::Result<Box<dyn ersa_adapter::Process>> {
        let change = ["add", "remove", "prune"].into_iter().find(|what| {
            spec.args
                .windows(2)
                .any(|pair| pair[0] == "worktree" && pair[1] == *what)
        });
        if let (true, Some(change)) = (spec.program == "git", change) {
            self.changes.lock().unwrap().push(change.to_string());
            if change == "add" {
                self.gate.stop().await;
            }
        }
        self.inner.spawn(spec).await
    }
}

/// A Core with `adapters`, whose `git worktree add`s stop at `gate` and
/// whose requests wait `wait` for a turn at a project's worktrees, with a
/// project in a new repository; its transport, a client and the project.
pub async fn with_stopping_adds(
    gate: &Arc<Gate>,
    wait: Duration,
    adapters: Vec<Arc<dyn ersa_core::Adapter>>,
) -> (super::TestCore, Arc<StoppingAdds>, Client, Value) {
    let git = Arc::new(StoppingAdds::new(gate));
    let transport = Arc::clone(&git);
    let configure = move |config: &mut ersa_core::CoreConfig| {
        config.command_transport = Some(ersa_core::CommandTransport(transport));
        config.worktree_turn_wait = wait;
    };
    let core = super::start_with_adapters(super::tempdir(), configure, adapters).await;
    let mut client = core.client().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let root = core.dir.path().join("worktrees").display().to_string();
    let patch = json!({ "patch": { "workspaces": { "root": root } } });
    client.result("settings.set", patch).await;
    let path = repo.display().to_string();
    let params = json!({ "host_id": "local", "path": path, "trusted": false });
    let project = client.result("project.add", params).await;
    (core, git, client, project)
}

/// A workspace made in `project`, its add let through `gate`.
pub async fn made_through(gate: &Gate, client: &mut Client, project: &Value) -> Value {
    let params = json!({ "project_id": project["id"] });
    let made = client.request("workspace.create", params).await;
    let reached = tokio::time::timeout(Duration::from_secs(30), gate.reached()).await;
    reached.expect("its add began");
    gate.go();
    let made = client.response(made).await["result"].clone();
    assert_eq!(made["state"], json!("ready"), "{made}");
    made
}

/// Asks `client` to make a workspace in `project`, and returns the request
/// once its add stands stopped at `gate`, holding the project's turn.
pub async fn hold_the_turn(gate: &Gate, client: &mut Client, project: &Value) -> i64 {
    let params = json!({ "project_id": project["id"] });
    let holding = client.request("workspace.create", params).await;
    let reached = tokio::time::timeout(Duration::from_secs(30), gate.reached()).await;
    reached.expect("the holder's add began");
    holding
}
