//! `workspace.changes`, what deleting a workspace would lose, driven over the
//! socket against temporary git repositories (PROTO-CORE-API-R38).

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ersa_core::Adapter;
use serde_json::{Value, json};
use support::fake::{self, FakeAdapter};
use support::{Client, TestCore, git, git_repo, git_repo_on, kind, start_with_adapters, tempdir};

/// A started Core with the fake backend, so sessions can be created, a
/// client, and `workspaces.root` pointed into the test's temporary
/// directory.
async fn setup() -> (TestCore, Client) {
    let adapters: Vec<Arc<dyn Adapter>> = vec![Arc::new(FakeAdapter::new(1))];
    let core = start_with_adapters(tempdir(), |_| {}, adapters).await;
    let mut client = core.client().await;
    let root = core.dir.path().join("worktrees").display().to_string();
    client
        .result(
            "settings.set",
            json!({ "patch": { "workspaces": { "root": root } } }),
        )
        .await;
    (core, client)
}

/// Adds the git repository at `repo` as a project and makes a workspace in
/// it; returns the workspace and its folder.
async fn workspace_of(client: &mut Client, repo: &Path) -> (Value, PathBuf) {
    let project = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false }),
        )
        .await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let folder = PathBuf::from(workspace["path"].as_str().unwrap());
    (workspace, folder)
}

/// `workspace.changes` for `workspace`. The client checks the answer
/// against the schema.
async fn changes(client: &mut Client, workspace: &Value) -> Value {
    client
        .result("workspace.changes", json!({ "id": workspace["id"] }))
        .await
}

/// `[uncommitted, ignored, unshared, stranded, sessions]`.
fn counts(changes: &Value) -> [u64; 5] {
    ["uncommitted", "ignored", "unshared", "stranded", "sessions"].map(|field| {
        changes[field]
            .as_u64()
            .unwrap_or_else(|| panic!("{changes}"))
    })
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["commit", "--quiet", "--allow-empty", "-m", message]);
}

/// A repository at `dir/repo` that ignores `*.log` and `build/`, with a
/// tracked file `old.txt` beside its README.
fn repo_with_ignores(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    git_repo(&repo);
    write(&repo.join(".gitignore"), "*.log\nbuild/\n");
    write(&repo.join("old.txt"), "old\n");
    git(&repo, &["add", ".gitignore", "old.txt"]);
    commit(&repo, "ignores");
    repo
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn counts_uncommitted_files_untracked_included_and_ignored_entries() {
    let (core, mut client) = setup().await;
    let repo = repo_with_ignores(core.dir.path());
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    let fresh = changes(&mut client, &workspace).await;
    assert_eq!(counts(&fresh), [0, 0, 0, 0, 0], "{fresh}");
    assert!(!fresh["token"].as_str().unwrap().is_empty());

    // Uncommitted: a modified tracked file, a staged new one, and three
    // untracked files, two of them in an untracked folder, each counted.
    write(&folder.join("README.md"), "changed\n");
    write(&folder.join("staged.txt"), "staged\n");
    git(&folder, &["add", "staged.txt"]);
    write(&folder.join("a.txt"), "a\n");
    write(&folder.join("notes/b.txt"), "b\n");
    write(&folder.join("notes/c.txt"), "c\n");
    // A tracked file deleted from the folder leaves nothing in it to lose.
    std::fs::remove_file(folder.join("old.txt")).unwrap();
    // Ignored: a file, a wholly ignored folder as one, and an ignored file
    // in an untracked folder.
    write(&folder.join("x.log"), "x\n");
    write(&folder.join("build/one"), "1\n");
    write(&folder.join("build/two"), "2\n");
    write(&folder.join("notes/d.log"), "d\n");

    let found = changes(&mut client, &workspace).await;
    assert_eq!(counts(&found), [5, 3, 0, 0, 0], "{found}");
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn the_token_changes_with_anything_counted_and_not_with_ignored_content() {
    let (core, mut client) = setup().await;
    let repo = repo_with_ignores(core.dir.path());
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    write(&folder.join("a.txt"), "a\n");
    write(&folder.join("x.log"), "x\n");
    write(&folder.join("build/one"), "1\n");
    let first = changes(&mut client, &workspace).await;
    assert_eq!(counts(&first), [1, 2, 0, 0, 0], "{first}");
    // Nothing changed, nothing new.
    let again = changes(&mut client, &workspace).await;
    assert_eq!(again["token"], first["token"]);

    // An ignored file's content, and a file added to a wholly ignored
    // folder, change no counted thing.
    write(&folder.join("x.log"), "x, longer\n");
    write(&folder.join("build/two"), "2\n");
    let ignored_edits = changes(&mut client, &workspace).await;
    assert_eq!(counts(&ignored_edits), [1, 2, 0, 0, 0]);
    assert_eq!(ignored_edits["token"], first["token"]);

    // Each change below changes the token, the content of an uncommitted
    // file included, which changes no count.
    let mut seen = vec![first["token"].clone()];
    let mut expect_new = |found: &Value| {
        assert!(
            !seen.contains(&found["token"]),
            "the token did not change: {found}"
        );
        seen.push(found["token"].clone());
    };
    write(&folder.join("a.txt"), "a, edited\n");
    let edited = changes(&mut client, &workspace).await;
    assert_eq!(counts(&edited), [1, 2, 0, 0, 0]);
    expect_new(&edited);

    write(&folder.join("y.log"), "y\n");
    let more_ignored = changes(&mut client, &workspace).await;
    assert_eq!(counts(&more_ignored), [1, 3, 0, 0, 0]);
    expect_new(&more_ignored);

    commit(&folder, "work");
    let committed = changes(&mut client, &workspace).await;
    assert_eq!(counts(&committed), [1, 3, 1, 0, 0]);
    expect_new(&committed);

    client
        .result(
            "session.create",
            json!({ "workspace_id": workspace["id"], "backend": fake::BACKEND }),
        )
        .await;
    let with_session = changes(&mut client, &workspace).await;
    assert_eq!(counts(&with_session), [1, 3, 1, 0, 1]);
    expect_new(&with_session);
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn counts_commits_no_other_branch_holds_from_the_branch_checkout_and_reflog() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    let branch = workspace["vcs_ref"].as_str().unwrap().to_string();

    // On the workspace's branch: unshared, and kept with the branch.
    commit(&folder, "on the branch");
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 1, 0, 0]
    );

    // On a detached checkout: held by no branch at all.
    git(&folder, &["checkout", "--quiet", "--detach"]);
    commit(&folder, "detached");
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 2, 1, 0]
    );
    let detached = git(&folder, &["rev-parse", "HEAD"]).trim().to_string();

    // Back on the branch, the reflog still names the detached commit.
    git(&folder, &["checkout", "--quiet", &branch]);
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 2, 1, 0]
    );

    // The branch reset past its commit: only the reflog has it now.
    git(&folder, &["reset", "--quiet", "--hard", "HEAD~1"]);
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 2, 2, 0]
    );

    // Another local branch holds both commits, the detached one's parent
    // being the branch's old commit: nothing is unshared.
    git(&repo, &["branch", "keep", &detached]);
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 0, 0, 0]
    );
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn a_commit_on_a_remote_tracking_branch_is_shared() {
    let (core, mut client) = setup().await;
    let upstream = core.dir.path().join("upstream");
    git_repo_on(&upstream, "main");
    let clone = core.dir.path().join("clone");
    git(
        core.dir.path(),
        &[
            "clone",
            "--quiet",
            &upstream.display().to_string(),
            &clone.display().to_string(),
        ],
    );
    let (workspace, folder) = workspace_of(&mut client, &clone).await;
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 0, 0, 0]
    );

    commit(&folder, "work");
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 1, 0, 0]
    );

    // Pushing the branch makes `origin/<branch>` hold the commit.
    git(&folder, &["push", "--quiet", "origin", "HEAD"]);
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 0, 0, 0]
    );
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn a_half_removed_workspace_is_counted_for_what_is_left() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    commit(&folder, "on the branch");
    git(&folder, &["checkout", "--quiet", "--detach"]);
    commit(&folder, "detached");
    write(&folder.join("a.txt"), "a\n");
    write(&folder.join("b.txt"), "b\n");
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [2, 0, 2, 1, 0]
    );

    // Partly gone, its link to the repository (`.git`) first.
    std::fs::remove_file(folder.join(".git")).unwrap();
    std::fs::remove_file(folder.join("a.txt")).unwrap();
    std::fs::remove_file(folder.join("README.md")).unwrap();
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [1, 0, 2, 1, 0]
    );

    // Wholly gone: git's own records still hold the detached checkout.
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 2, 1, 0]
    );
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn a_folder_git_does_not_list_is_refused() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    // git forgets the worktree, leaving its folder: git can no longer say
    // what in it is uncommitted.
    let name = folder.file_name().unwrap();
    std::fs::remove_dir_all(repo.join(".git").join("worktrees").join(name)).unwrap();
    let error = client
        .error("workspace.changes", json!({ "id": workspace["id"] }))
        .await;
    assert_eq!(kind(&error), "vcs_error", "{error}");
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn a_root_workspace_is_refused() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let project = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false }),
        )
        .await;
    let listed = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    let error = client
        .error("workspace.changes", json!({ "id": listed[0]["id"] }))
        .await;
    assert_eq!(kind(&error), "invalid_state", "{error}");

    let error = client
        .error("workspace.changes", json!({ "id": "w-nope" }))
        .await;
    assert_eq!(kind(&error), "not_found", "{error}");
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn a_detached_checkout_counts_without_a_reflog() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    git(&folder, &["checkout", "--quiet", "--detach"]);
    commit(&folder, "detached");
    // A reflog may be expired, or never kept; the checkout still holds the
    // commit.
    git(&folder, &["reflog", "expire", "--expire=now", "--all"]);
    assert_eq!(git(&folder, &["reflog", "show", "HEAD"]), "");
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 1, 1, 0]
    );
}
