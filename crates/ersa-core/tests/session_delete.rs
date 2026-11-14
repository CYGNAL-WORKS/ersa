//! `session.delete`, the user's Delete (PROTO-CORE-API, Sessions;
//! FLOW-ARCHIVE-DELETE-SESSION), driven over the socket against temporary
//! git repositories, with the fake backend's record standing in for Claude
//! Code's (support/fake.rs).

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ersa_adapter::OpenElsewhere;
use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter, Records};
use support::{Client, TestCore, git, git_repo, kind, start_with_adapters, tempdir};

/// A started Core with `fake` as its backend, and a client.
async fn start(fake: &Arc<FakeAdapter>) -> (TestCore, Client) {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::clone(fake) as Arc<dyn Adapter>];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let client = core.client().await;
    (core, client)
}

/// Restarts `core` on the same data directory with a fake backend that
/// keeps its records in `records`, as a user closing and opening Ersa would.
async fn restarted(core: TestCore, records: Records) -> (TestCore, Client) {
    let mut client = core.client().await;
    client.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::with_records(records))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let client = core.client().await;
    (core, client)
}

/// A git repository at `dir/repo`, added as a project; returns the project
/// and the repository's folder.
async fn project(client: &mut Client, dir: &Path) -> (Value, PathBuf) {
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
async fn isolated(client: &mut Client, project: &Value) -> (Value, PathBuf) {
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let folder = PathBuf::from(workspace["path"].as_str().unwrap());
    (workspace, folder)
}

/// The project's root workspace.
async fn root(client: &mut Client, project: &Value) -> Value {
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
async fn idle_session(client: &mut Client, workspace: &Value) -> String {
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
async fn wait_for_state(client: &mut Client, id: &str, state: &str) {
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
async fn listed(client: &mut Client, id: &str) -> Option<Value> {
    let sessions = client.result("session.list", json!({})).await;
    sessions
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
        .cloned()
}

/// The token of the workspace's `workspace.changes` now.
async fn token(client: &mut Client, workspace: &Value) -> Value {
    client
        .result("workspace.changes", json!({ "id": workspace["id"] }))
        .await["token"]
        .clone()
}

/// Whether `workspace.list` still lists the workspace.
async fn workspace_listed(client: &mut Client, project: &Value, workspace: &Value) -> bool {
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    workspaces
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w["id"] == workspace["id"])
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    !git(repo, &["branch", "--list", branch]).trim().is_empty()
}

fn branch_of(workspace: &Value) -> String {
    workspace["vcs_ref"].as_str().unwrap().to_string()
}

/// The backend's own id for the session, which keys its record.
fn backend_ids(records: &Records) -> Vec<String> {
    let mut ids: Vec<String> = records.lock().unwrap().keys().cloned().collect();
    ids.sort();
    ids
}

// verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R09
// verifies: PROTO-CORE-API-R35
// verifies: PROTO-CORE-API-R36
#[tokio::test(flavor = "multi_thread")]
async fn a_session_alone_in_its_workspace_goes_with_its_folder_branch_and_record() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    assert_eq!(backend_ids(&fake.records).len(), 1);
    let branch = branch_of(&workspace);
    assert!(folder.is_dir() && branch_exists(&repo, &branch));

    let token = token(&mut client, &workspace).await;
    let response = client
        .call("session.delete", json!({ "id": id, "token": token }))
        .await;
    support::assert_valid("session.delete", &response);
    assert_eq!(
        response["result"],
        json!({ "id": id, "workspace_deleted": true, "branch_kept": false })
    );
    // It was stopped first, then everything went: the folder, the branch,
    // the backend's record, the session and the workspace.
    assert_eq!(fake.stats.stops.load(Ordering::SeqCst), 1);
    assert!(!folder.exists());
    assert!(!branch_exists(&repo, &branch));
    assert!(backend_ids(&fake.records).is_empty());
    assert_eq!(fake.stats.record_deletes.load(Ordering::SeqCst), 1);
    assert!(listed(&mut client, &id).await.is_none());
    assert!(!workspace_listed(&mut client, &project, &workspace).await);
    // The project's own folder and branch are as they were.
    assert!(repo.join("README.md").is_file());
    assert!(branch_exists(&repo, "main"));

    // Never listed again, across a restart either.
    let records = Arc::clone(&fake.records);
    let (_core, mut client) = restarted(core, records).await;
    assert!(listed(&mut client, &id).await.is_none());
}

// verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
// verifies: PROTO-CORE-API-R35
#[tokio::test(flavor = "multi_thread")]
async fn a_branch_holding_commits_found_nowhere_else_is_kept() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    git(
        &folder,
        &["commit", "--quiet", "--allow-empty", "-m", "work"],
    );
    let id = idle_session(&mut client, &workspace).await;

    let token = token(&mut client, &workspace).await;
    let deleted = client
        .result("session.delete", json!({ "id": id, "token": token }))
        .await;
    assert_eq!(deleted["workspace_deleted"], true);
    assert_eq!(deleted["branch_kept"], true);
    assert!(!folder.exists());
    let branch = branch_of(&workspace);
    assert!(branch_exists(&repo, &branch), "the branch holds the work");
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%s", &branch]).trim(),
        "work"
    );
}

// verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
// verifies: PROTO-CORE-API-R35
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_another_session_uses_keeps_its_files() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let deleted = idle_session(&mut client, &workspace).await;
    let kept = idle_session(&mut client, &workspace).await;
    std::fs::write(folder.join("notes.txt"), "mine\n").unwrap();

    let token = token(&mut client, &workspace).await;
    let result = client
        .result("session.delete", json!({ "id": deleted, "token": token }))
        .await;
    assert_eq!(
        result,
        json!({ "id": deleted, "workspace_deleted": false, "branch_kept": false })
    );
    assert!(folder.join("notes.txt").is_file());
    assert!(branch_exists(&repo, &branch_of(&workspace)));
    assert!(workspace_listed(&mut client, &project, &workspace).await);
    assert!(listed(&mut client, &deleted).await.is_none());
    assert!(listed(&mut client, &kept).await.is_some());
    // Only the deleted session's record went.
    assert_eq!(backend_ids(&fake.records).len(), 1);
}

// verifies: FLOW-ARCHIVE-DELETE-SESSION-R06
// verifies: PROTO-CORE-API-R35
#[tokio::test(flavor = "multi_thread")]
async fn a_root_workspace_is_never_touched_and_needs_no_token() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let root = root(&mut client, &project).await;
    let id = idle_session(&mut client, &root).await;
    // Work of every kind in the project's own folder.
    std::fs::write(repo.join("untracked.txt"), "keep\n").unwrap();
    std::fs::write(repo.join("README.md"), "changed\n").unwrap();
    git(&repo, &["checkout", "--quiet", "-b", "topic"]);
    git(
        &repo,
        &["commit", "--quiet", "--allow-empty", "-m", "topic work"],
    );
    let status = git(&repo, &["status", "--porcelain"]);

    let result = client.result("session.delete", json!({ "id": id })).await;
    assert_eq!(
        result,
        json!({ "id": id, "workspace_deleted": false, "branch_kept": false })
    );
    assert_eq!(git(&repo, &["status", "--porcelain"]), status);
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "topic"
    );
    assert!(branch_exists(&repo, "main"));
    assert!(workspace_listed(&mut client, &project, &root).await);
    assert!(listed(&mut client, &id).await.is_none());
    assert!(backend_ids(&fake.records).is_empty());
}

// verifies: FLOW-ARCHIVE-DELETE-SESSION-R08
// verifies: PROTO-CORE-API-R40
// verifies: PROTO-CORE-API-R53
#[tokio::test(flavor = "multi_thread")]
async fn a_stale_token_is_refused_with_the_new_changes_after_the_stop_and_nothing_goes() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    let shown = token(&mut client, &workspace).await;
    // Something changes after the question: a file, even with the same
    // counts as some earlier state.
    std::fs::write(folder.join("late.txt"), "late\n").unwrap();

    let response = client
        .call("session.delete", json!({ "id": id, "token": shown }))
        .await;
    support::assert_valid("session.delete", &response);
    let error = &response["error"];
    assert_eq!(kind(error), "invalid_state", "{error}");
    let now: Value = serde_json::from_str(error["data"]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(now["uncommitted"], 1, "{now}");
    assert_eq!(now["sessions"], 1, "{now}");
    assert_ne!(now["token"], shown);
    assert_eq!(now["token"], token(&mut client, &workspace).await);

    // Stopped, and otherwise as it was: nothing deleted, nothing marked.
    assert_eq!(fake.stats.stops.load(Ordering::SeqCst), 1);
    let row = listed(&mut client, &id).await.expect("still listed");
    assert_eq!(row["state"], "ended");
    assert!(row.get("deleting").is_none(), "{row}");
    assert!(folder.join("late.txt").is_file());
    assert!(branch_exists(&repo, &branch_of(&workspace)));
    assert_eq!(backend_ids(&fake.records).len(), 1);

    // With the token of what is there now, it goes.
    let result = client
        .result("session.delete", json!({ "id": id, "token": now["token"] }))
        .await;
    assert_eq!(result["workspace_deleted"], true);
    assert!(!folder.exists());
}

// verifies: PROTO-CORE-API-R54
#[tokio::test(flavor = "multi_thread")]
async fn a_session_in_an_isolated_workspace_needs_a_token() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, _repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    let error = client.error("session.delete", json!({ "id": id })).await;
    assert_eq!(error["code"], -32602, "{error}");
    assert_eq!(fake.stats.stops.load(Ordering::SeqCst), 0);
    assert!(folder.is_dir());

    // Before any other refusal: a session with a turn running, which would
    // otherwise be refused `invalid_state`, answers `invalid_params` first.
    let busy = idle_session(&mut client, &workspace).await;
    client
        .result("session.send", json!({ "id": busy, "text": "work" }))
        .await;
    let error = client.error("session.delete", json!({ "id": busy })).await;
    assert_eq!(error["code"], -32602, "{error}");
    assert_eq!(fake.stats.stops.load(Ordering::SeqCst), 0);
    assert!(folder.is_dir());
}

// verifies: FLOW-ARCHIVE-DELETE-SESSION-R05
// verifies: PROTO-CORE-API-R35
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn refused_while_the_session_is_starting_or_a_turn_is_running() {
    // Starting: the backend takes a while to become ready.
    let fake = Arc::new(FakeAdapter::slow_start(Duration::from_secs(2)));
    let (core, mut client) = start(&fake).await;
    let (proj, _repo) = project(&mut client, core.dir.path()).await;
    let root_ws = root(&mut client, &proj).await;
    let starting = client
        .result(
            "session.create",
            json!({ "workspace_id": root_ws["id"], "backend": fake::BACKEND }),
        )
        .await;
    assert_eq!(starting["state"], "starting");
    let error = client
        .error("session.delete", json!({ "id": starting["id"] }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error}");
    assert_eq!(error["data"]["reason"], "session_busy", "{error}");
    assert!(
        error["data"].get("detail").is_none(),
        "refused before any claim"
    );

    // A turn running, and one waiting on the user.
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (proj, _repo) = project(&mut client, core.dir.path()).await;
    let root_ws = root(&mut client, &proj).await;
    for (text, state) in [("work", "running"), ("ask", "waiting_user")] {
        let id = idle_session(&mut client, &root_ws).await;
        client
            .result("session.send", json!({ "id": id, "text": text }))
            .await;
        if text == "ask" {
            client.events_until("permission.requested").await;
        }
        let error = client.error("session.delete", json!({ "id": id })).await;
        assert_eq!(kind(&error), "invalid_state", "{state}: {error}");
        assert_eq!(error["data"]["reason"], "session_busy", "{state}: {error}");
        let row = listed(&mut client, &id).await.expect("still listed");
        assert!(row.get("deleting").is_none());
        assert_ne!(row["state"], "ended", "{state}: not stopped");
    }
    assert_eq!(fake.stats.stops.load(Ordering::SeqCst), 0);
    assert_eq!(fake.stats.record_deletes.load(Ordering::SeqCst), 0);
}

/// While a delete holds its workspace, what would change the session or
/// the workspace is refused `busy`; once it is done, the claim is gone.
#[tokio::test(flavor = "multi_thread")]
async fn the_claim_holds_off_other_requests_until_the_delete_ends() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, _repo) = project(&mut client, core.dir.path()).await;
    let (workspace, _folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    let other = idle_session(&mut client, &workspace).await;
    let shown = token(&mut client, &workspace).await;

    // The registry check, after the claim and the stop, waits on the gate.
    let gate = Arc::clone(&fake.open_check_gate);
    let held = gate.lock().await;
    let pending = client
        .request("session.delete", json!({ "id": id, "token": shown }))
        .await;
    // Until the stop has run, the claim is certainly taken.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while fake.stats.stops.load(Ordering::SeqCst) == 0 {
        assert!(tokio::time::Instant::now() < deadline, "never stopped");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let mut second = core.client().await;
    let refusals = [
        ("session.send", json!({ "id": id, "text": "hello" })),
        ("session.delete", json!({ "id": other, "token": shown })),
        ("session.remove", json!({ "id": other })),
        ("session.fork", json!({ "id": id })),
        ("session.fork", json!({ "id": other })),
        (
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        ),
    ];
    for (method, params) in refusals {
        let response = second.call(method, params).await;
        support::assert_valid(method, &response);
        let error = &response["error"];
        assert_eq!(kind(error), "invalid_state", "{method}: {response}");
        assert_eq!(error["data"]["detail"], "busy", "{method}: {response}");
    }
    // The refused create left no session behind.
    let sessions = second.result("session.list", json!({})).await;
    assert_eq!(sessions.as_array().unwrap().len(), 2);

    drop(held);
    let response = client.response(pending).await;
    assert_eq!(response["result"]["workspace_deleted"], false, "{response}");

    // Released: the workspace takes a new session again.
    second
        .result(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;
}

/// Another process with the session open, or a registry that cannot say,
/// refuses the delete with nothing deleted or marked, and releases the
/// claim.
// verifies: PROTO-CORE-API-R52
// verifies: PROTO-CORE-API-R56
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn refused_with_nothing_deleted_when_another_process_has_the_session_open() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;

    for answer in [
        OpenElsewhere::Yes { pids: vec![4242] },
        OpenElsewhere::Unknown,
    ] {
        *fake.open_elsewhere.lock().unwrap() = answer.clone();
        let shown = token(&mut client, &workspace).await;
        let error = client
            .error("session.delete", json!({ "id": id, "token": shown }))
            .await;
        assert_eq!(kind(&error), "invalid_state", "{error}");
        if answer == OpenElsewhere::Unknown {
            assert_eq!(error["data"]["reason"], "open_unknown", "{error}");
        } else {
            assert_eq!(error["data"]["reason"], "open_elsewhere", "{error}");
            // `detail` names the processes, for the Interface to show.
            assert!(
                error["data"]["detail"].as_str().unwrap().contains("4242"),
                "{error}"
            );
            assert!(
                error["message"].as_str().unwrap().contains("4242"),
                "{error}"
            );
        }
        // Stopped before the registry was asked, and left stopped.
        assert_eq!(fake.stats.stops.load(Ordering::SeqCst), 1);
        let checks = fake.stats.open_checks.lock().unwrap().len();
        assert!(checks >= 1, "the registry was asked");
        let row = listed(&mut client, &id).await.expect("still listed");
        assert_eq!(row["state"], "ended", "{row}");
        assert!(row.get("deleting").is_none(), "{row}");
        assert!(folder.is_dir());
        assert!(branch_exists(&repo, &branch_of(&workspace)));
        assert_eq!(backend_ids(&fake.records).len(), 1);
    }
    // The claim went with the refusal.
    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;
}

/// A session whose backend does not stop ends the delete with the stop's
/// error, deleting and marking nothing. Takes Core's 30-second stop limit.
#[tokio::test(flavor = "multi_thread")]
async fn a_stop_that_times_out_ends_the_delete() {
    let fake = Arc::new(FakeAdapter::unstoppable());
    let (core, mut client) = start(&fake).await;
    let (project, _repo) = project(&mut client, core.dir.path()).await;
    let root = root(&mut client, &project).await;
    let id = idle_session(&mut client, &root).await;

    let error = client.error("session.delete", json!({ "id": id })).await;
    assert_eq!(kind(&error), "internal", "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("did not stop"),
        "{error}"
    );
    let row = listed(&mut client, &id).await.expect("still listed");
    assert!(row.get("deleting").is_none(), "{row}");
    assert_eq!(fake.stats.record_deletes.load(Ordering::SeqCst), 0);
}

/// A failed step leaves the session marked `deleting: failed`, refused
/// what would change it and left out of `workspace.changes`; a new delete
/// with a fresh token finishes it.
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
// verifies: PROTO-CORE-API-R35
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_step_reads_failed_and_a_fresh_token_finishes_it() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    // A locked worktree refuses removal.
    git(&repo, &["worktree", "lock", folder.to_str().unwrap()]);

    let shown = token(&mut client, &workspace).await;
    let error = client
        .error("session.delete", json!({ "id": id, "token": shown }))
        .await;
    assert_eq!(kind(&error), "vcs_error", "{error}");
    let row = listed(&mut client, &id).await.expect("kept");
    assert_eq!(row["deleting"], "failed");
    assert_eq!(row["state"], "ended");
    assert!(folder.is_dir());
    assert_eq!(backend_ids(&fake.records).len(), 1, "the record stays");

    // Marked deleting: refused what would change it.
    for (method, params) in [
        ("session.send", json!({ "id": id, "text": "hello" })),
        (
            "session.set_archived",
            json!({ "id": id, "archived": true }),
        ),
        ("session.arrange", json!({ "id": id, "pinned": true })),
        ("session.remove", json!({ "id": id })),
    ] {
        let error = client.error(method, params).await;
        assert_eq!(kind(&error), "invalid_state", "{method}: {error}");
        assert_eq!(
            error["data"]["reason"], "session_deleting",
            "{method}: {error}"
        );
    }

    // The old token is stale now, since the session is no longer counted.
    let changes = client
        .result("workspace.changes", json!({ "id": workspace["id"] }))
        .await;
    assert_eq!(changes["sessions"], 0, "{changes}");
    let error = client
        .error("session.delete", json!({ "id": id, "token": shown }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error}");
    assert_eq!(
        listed(&mut client, &id).await.unwrap()["deleting"],
        "failed"
    );

    git(&repo, &["worktree", "unlock", folder.to_str().unwrap()]);
    let fresh = token(&mut client, &workspace).await;
    let result = client
        .result("session.delete", json!({ "id": id, "token": fresh }))
        .await;
    assert_eq!(result["workspace_deleted"], true);
    assert!(!folder.exists());
    assert!(!branch_exists(&repo, &branch_of(&workspace)));
    assert!(listed(&mut client, &id).await.is_none());
    assert!(backend_ids(&fake.records).is_empty());
}

/// A backend that refuses to delete the record while saying no process has
/// the session open broke its contract: the delete fails `internal`, never
/// with a reason the user could act on.
#[tokio::test(flavor = "multi_thread")]
async fn a_record_deletion_refused_with_nothing_open_fails_internal() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, _repo) = project(&mut client, core.dir.path()).await;
    let (workspace, _folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    *fake.delete_refusal.lock().unwrap() = Some(OpenElsewhere::No);

    let shown = token(&mut client, &workspace).await;
    let error = client
        .error("session.delete", json!({ "id": id, "token": shown }))
        .await;
    assert_eq!(kind(&error), "internal", "{error}");
    assert!(error["data"].get("reason").is_none(), "{error}");
    assert_eq!(
        listed(&mut client, &id).await.unwrap()["deleting"],
        "failed"
    );
}

/// A record the registry shows newly open when it is deleted fails the
/// delete after the files went; trying again with what is left finishes it.
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R07
// verifies: FLOW-ARCHIVE-DELETE-SESSION-R09
// verifies: PROTO-CORE-API-R79
#[tokio::test(flavor = "multi_thread")]
async fn a_record_opened_elsewhere_part_way_fails_the_delete_until_tried_again() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    *fake.delete_refusal.lock().unwrap() = Some(OpenElsewhere::Yes { pids: vec![777] });

    let shown = token(&mut client, &workspace).await;
    let error = client
        .error("session.delete", json!({ "id": id, "token": shown }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error}");
    assert_eq!(error["data"]["reason"], "open_elsewhere", "{error}");
    assert!(
        error["data"]["detail"].as_str().unwrap().contains("777"),
        "{error}"
    );
    assert!(
        error["message"].as_str().unwrap().contains("777"),
        "{error}"
    );
    assert_eq!(
        listed(&mut client, &id).await.unwrap()["deleting"],
        "failed"
    );
    assert!(!folder.exists(), "the files had gone");
    assert_eq!(backend_ids(&fake.records).len(), 1);
    // Its files gone, the workspace takes no new session.
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    let listed_workspace = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == workspace["id"])
        .cloned()
        .unwrap();
    assert_eq!(listed_workspace["state"], "error");
    let error = client
        .error(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error}");
    // Nor a fork of the session marked deleting in it.
    let error = client.error("session.fork", json!({ "id": id })).await;
    assert_eq!(kind(&error), "invalid_state", "{error}");
    assert_eq!(error["data"]["reason"], "session_deleting", "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("being deleted"),
        "{error}"
    );

    // What is left: nothing of the folder, nor the branch.
    *fake.delete_refusal.lock().unwrap() = None;
    let fresh = token(&mut client, &workspace).await;
    let result = client
        .result("session.delete", json!({ "id": id, "token": fresh }))
        .await;
    assert_eq!(
        result,
        json!({ "id": id, "workspace_deleted": true, "branch_kept": false })
    );
    assert!(!branch_exists(&repo, &branch_of(&workspace)));
    assert!(backend_ids(&fake.records).is_empty());
    assert!(!workspace_listed(&mut client, &project, &workspace).await);
}

/// Core restarting part way through a delete marks it failed and deletes
/// nothing more; whether the files go was recorded at the mark and is
/// never widened, and a new delete with a fresh token finishes it.
// verifies: PROTO-CORE-API-R40
#[tokio::test(flavor = "multi_thread")]
async fn a_restart_marks_a_running_delete_failed_and_deletes_nothing_more() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    client.result("session.stop", json!({ "id": id })).await;

    // A delete under way when Core went: marked running, the files to stay
    // (as they would for a workspace shared at the time of the mark).
    let db = core.data_dir().join("ersa.db");
    let records = Arc::clone(&fake.records);
    let mut client_gone = client;
    client_gone.result("core.shutdown", json!({})).await;
    let TestCore { dir, task, .. } = core;
    task.await.unwrap().unwrap();
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let marked = conn
            .execute(
                "UPDATE sessions SET deleting = 'running', delete_files = 0 WHERE id = ?1",
                [&id],
            )
            .unwrap();
        assert_eq!(marked, 1);
    }
    let adapters: Vec<Arc<dyn Adapter>> =
        vec![Arc::new(FakeAdapter::with_records(Arc::clone(&records)))];
    let core = start_with_adapters(dir, |_| {}, adapters).await;
    let mut client = core.client().await;

    let row = listed(&mut client, &id).await.expect("listed");
    assert_eq!(row["deleting"], "failed");
    assert!(folder.is_dir());
    assert_eq!(backend_ids(&records).len(), 1, "nothing more was deleted");

    // Tried again, now alone in the workspace: the files still stay, as
    // recorded, and the rest goes.
    let fresh = token(&mut client, &workspace).await;
    let result = client
        .result("session.delete", json!({ "id": id, "token": fresh }))
        .await;
    assert_eq!(
        result,
        json!({ "id": id, "workspace_deleted": false, "branch_kept": false })
    );
    assert!(folder.is_dir());
    assert!(branch_exists(&repo, &branch_of(&workspace)));
    assert!(backend_ids(&records).is_empty());
    assert!(listed(&mut client, &id).await.is_none());
}

/// `workspace.changes` leaves a session marked deleting out of `sessions`
/// and out of its token.
// verifies: PROTO-CORE-API-R35
#[tokio::test(flavor = "multi_thread")]
async fn workspace_changes_leaves_out_a_session_marked_deleting() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, _repo) = project(&mut client, core.dir.path()).await;
    let (workspace, _folder) = isolated(&mut client, &project).await;
    let marked = idle_session(&mut client, &workspace).await;
    let _counted = idle_session(&mut client, &workspace).await;
    let before = client
        .result("workspace.changes", json!({ "id": workspace["id"] }))
        .await;
    assert_eq!(before["sessions"], 2);

    // Its record refuses to go, so the delete stops marked failed.
    *fake.delete_refusal.lock().unwrap() = Some(OpenElsewhere::Unknown);
    let error = client
        .error(
            "session.delete",
            json!({ "id": marked, "token": before["token"] }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error}");
    assert_eq!(
        listed(&mut client, &marked).await.unwrap()["deleting"],
        "failed"
    );

    let after = client
        .result("workspace.changes", json!({ "id": workspace["id"] }))
        .await;
    assert_eq!(after["sessions"], 1, "{after}");
    assert_ne!(after["token"], before["token"]);
}

/// When `workspace.changes` fails in step 3, the delete answers with its
/// error, releases the claim, and has deleted and marked nothing.
// verifies: PROTO-CORE-API-R50
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_check_of_the_changes_releases_the_claim_and_deletes_nothing() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    let shown = token(&mut client, &workspace).await;
    // git can no longer find the repository.
    let hidden = repo.with_file_name("hidden-git");
    std::fs::rename(repo.join(".git"), &hidden).unwrap();

    let error = client
        .error("session.delete", json!({ "id": id, "token": shown }))
        .await;
    assert_eq!(kind(&error), "vcs_error", "{error}");
    let row = listed(&mut client, &id).await.expect("still listed");
    assert!(row.get("deleting").is_none(), "{row}");
    assert!(folder.is_dir());
    assert_eq!(backend_ids(&fake.records).len(), 1);
    // The claim went with the error.
    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;

    std::fs::rename(&hidden, repo.join(".git")).unwrap();
    assert!(branch_exists(&repo, &branch_of(&workspace)));
}

/// A session whose delete kept the shared workspace's files, then left
/// behind when another session's delete took them, takes the workspace's
/// row with it when its own delete finishes: no workspace is left listed
/// with its folder gone.
// verifies: PROTO-CORE-API-R35
// verifies: PROTO-CORE-API-R55
#[tokio::test(flavor = "multi_thread")]
async fn the_last_session_out_of_a_workspace_whose_files_went_takes_its_row() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, _repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let first = idle_session(&mut client, &workspace).await;
    let second = idle_session(&mut client, &workspace).await;

    // The first delete, shared, keeps the files and fails at the record.
    *fake.delete_refusal.lock().unwrap() = Some(OpenElsewhere::Unknown);
    let shown = token(&mut client, &workspace).await;
    client
        .error("session.delete", json!({ "id": first, "token": shown }))
        .await;
    *fake.delete_refusal.lock().unwrap() = None;

    // The second, counted alone, takes the files; the first still refers
    // to the workspace, so its row stays for now.
    let shown = token(&mut client, &workspace).await;
    let result = client
        .result("session.delete", json!({ "id": second, "token": shown }))
        .await;
    assert_eq!(result["workspace_deleted"], true);
    assert!(!folder.exists());
    assert!(workspace_listed(&mut client, &project, &workspace).await);

    // The first, tried again, keeps the files as recorded, and being the
    // last out of a workspace whose folder is gone, takes the row.
    let shown = token(&mut client, &workspace).await;
    let result = client
        .result("session.delete", json!({ "id": first, "token": shown }))
        .await;
    assert_eq!(result["workspace_deleted"], false);
    assert!(!workspace_listed(&mut client, &project, &workspace).await);
    assert!(backend_ids(&fake.records).is_empty());
}

/// A folder missing for some other reason (a drive unplugged, a folder
/// deleted by hand) while git still lists the worktree keeps the row: Ersa
/// did not delete those files, and their worktree and branch stay findable.
// verifies: PROTO-CORE-API-R55
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_missing_while_git_still_lists_it_keeps_the_row() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, _repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let first = idle_session(&mut client, &workspace).await;
    let second = idle_session(&mut client, &workspace).await;

    // Shared: the first delete keeps the files, and fails at the record.
    *fake.delete_refusal.lock().unwrap() = Some(OpenElsewhere::Unknown);
    let shown = token(&mut client, &workspace).await;
    client
        .error("session.delete", json!({ "id": first, "token": shown }))
        .await;
    *fake.delete_refusal.lock().unwrap() = None;
    client.result("session.stop", json!({ "id": second })).await;
    client
        .result("session.remove", json!({ "id": second }))
        .await;
    std::fs::remove_dir_all(&folder).unwrap();

    let shown = token(&mut client, &workspace).await;
    let result = client
        .result("session.delete", json!({ "id": first, "token": shown }))
        .await;
    assert_eq!(result["workspace_deleted"], false);
    assert!(workspace_listed(&mut client, &project, &workspace).await);
}

/// Once the session is marked deleting with its files to go, the workspace
/// is `error`, before its folder is touched: here a locked worktree stops
/// the delete at step 5 with every file in place, and the workspace still
/// takes no new session. The finished delete removes its row.
// verifies: PROTO-CORE-API-R57
#[tokio::test(flavor = "multi_thread")]
async fn a_workspace_leaves_ready_when_the_session_is_marked_before_its_folder_is_touched() {
    let fake = Arc::new(FakeAdapter::new(1));
    let (core, mut client) = start(&fake).await;
    let (project, repo) = project(&mut client, core.dir.path()).await;
    let (workspace, folder) = isolated(&mut client, &project).await;
    let id = idle_session(&mut client, &workspace).await;
    std::fs::write(folder.join("kept.txt"), "kept\n").unwrap();
    git(&repo, &["worktree", "lock", folder.to_str().unwrap()]);

    let shown = token(&mut client, &workspace).await;
    let error = client
        .error("session.delete", json!({ "id": id, "token": shown }))
        .await;
    assert_eq!(kind(&error), "vcs_error", "{error}");
    assert!(folder.join("kept.txt").is_file(), "the folder is untouched");
    assert_eq!(
        listed(&mut client, &id).await.unwrap()["deleting"],
        "failed"
    );
    let workspaces = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    let state = workspaces
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == workspace["id"])
        .map(|w| w["state"].clone());
    assert_eq!(state, Some(json!("error")));
    let error = client
        .error(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error}");

    git(&repo, &["worktree", "unlock", folder.to_str().unwrap()]);
    let fresh = token(&mut client, &workspace).await;
    client
        .result("session.delete", json!({ "id": id, "token": fresh }))
        .await;
    assert!(!workspace_listed(&mut client, &project, &workspace).await);
}
