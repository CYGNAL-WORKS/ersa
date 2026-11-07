//! `workspace.changes`, what deleting a workspace would lose, driven over the
//! socket against temporary git repositories (PROTO-CORE-API-R38).

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

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
    // A tracked file deleted from the folder leaves nothing in it to lose;
    // one staged and then deleted leaves its staged content in the index,
    // which goes with the workspace.
    std::fs::remove_file(folder.join("old.txt")).unwrap();
    write(&folder.join("gone.txt"), "staged, then deleted\n");
    git(&folder, &["add", "gone.txt"]);
    std::fs::remove_file(folder.join("gone.txt")).unwrap();
    // Ignored: a file, a wholly ignored folder as one, and an ignored file
    // in an untracked folder.
    write(&folder.join("x.log"), "x\n");
    write(&folder.join("build/one"), "1\n");
    write(&folder.join("build/two"), "2\n");
    write(&folder.join("notes/d.log"), "d\n");

    let found = changes(&mut client, &workspace).await;
    assert_eq!(counts(&found), [6, 3, 0, 0, 0], "{found}");
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
async fn files_left_after_git_forgot_the_worktree_all_count() {
    let (core, mut client) = setup().await;
    let repo = repo_with_ignores(core.dir.path());
    // Committed, and with no line ending for git to convert, so it is
    // unchanged against the repository's own index on every platform.
    write(&repo.join("same.txt"), "same");
    git(&repo, &["add", "same.txt"]);
    commit(&repo, "same");
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    write(&folder.join("a.txt"), "a\n");
    write(&folder.join("x.log"), "x\n");
    // A removal that failed on Windows: git deleted its record of the
    // worktree, and a file it could not delete is left with the rest.
    let name = folder.file_name().unwrap();
    std::fs::remove_dir_all(repo.join(".git").join("worktrees").join(name)).unwrap();
    std::fs::remove_file(folder.join("old.txt")).unwrap();
    // git cannot say which were committed, so every file left counts:
    // README.md, .gitignore, same.txt and a.txt (git never lists the
    // `.git` file); ignored ones are still ignored.
    let found = changes(&mut client, &workspace).await;
    assert_eq!(counts(&found), [4, 1, 0, 0, 0], "{found}");
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn a_worktree_recorded_with_relative_paths_is_found() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    // git 2.48 and later then write the record's `gitdir` relative to it;
    // older versions ignore the setting.
    git(&repo, &["config", "worktree.useRelativePaths", "true"]);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    git(&folder, &["checkout", "--quiet", "--detach"]);
    commit(&folder, "detached");
    write(&folder.join("a.txt"), "a\n");
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [1, 0, 1, 1, 0]
    );
    std::fs::remove_dir_all(&folder).unwrap();
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 1, 1, 0]
    );
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

// cites PROTO-CORE-API-R46 once planning PR 245 adds it
#[tokio::test(flavor = "multi_thread")]
async fn without_git_s_record_only_the_branch_s_commits_count() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    let branch = workspace["vcs_ref"].as_str().unwrap().to_string();
    commit(&folder, "on the branch");
    git(&folder, &["checkout", "--quiet", "--detach"]);
    commit(&folder, "detached");
    git(&folder, &["checkout", "--quiet", &branch]);
    write(&folder.join("a.txt"), "a\n");
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [1, 0, 2, 1, 0]
    );

    // git deletes its record: the checkout and its reflog go with it.
    let name = folder.file_name().unwrap();
    std::fs::remove_dir_all(repo.join(".git").join("worktrees").join(name)).unwrap();
    let forgotten = changes(&mut client, &workspace).await;
    // README.md and a.txt left; the branch's commit; nothing stranded.
    assert_eq!(counts(&forgotten), [2, 0, 1, 0, 0], "{forgotten}");

    // The repository's own checkout moving changes nothing counted, so it
    // leaves the token alone.
    write(&repo.join("README.md"), "changed elsewhere");
    git(&repo, &["commit", "--quiet", "-am", "elsewhere"]);
    let again = changes(&mut client, &workspace).await;
    assert_eq!(counts(&again), [2, 0, 1, 0, 0]);
    assert_eq!(again["token"], forgotten["token"]);
}

/// A repository at `dir/<name>` cloned from a new one at `dir/<name>-upstream`,
/// so its one commit is on its remote.
fn pushed_clone(dir: &Path, name: &str) -> PathBuf {
    let upstream = dir.join(format!("{name}-upstream"));
    git_repo(&upstream);
    let clone = dir.join(name);
    git(
        dir,
        &[
            "clone",
            "--quiet",
            &upstream.display().to_string(),
            &clone.display().to_string(),
        ],
    );
    clone
}

// cites PROTO-CORE-API-R49 once planning PR 245 adds it
#[tokio::test(flavor = "multi_thread")]
async fn a_nested_repository_counts_once_when_it_holds_work() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;

    // Clean, and its commit on its remote: nothing to lose.
    let nested = folder.join("nested");
    git(
        &folder,
        &[
            "clone",
            "--quiet",
            &pushed_clone(core.dir.path(), "source")
                .display()
                .to_string(),
            "nested",
        ],
    );
    let clean = changes(&mut client, &workspace).await;
    assert_eq!(counts(&clean), [0, 0, 0, 0, 0], "{clean}");

    // A change inside it counts it, once, however many files changed.
    write(&nested.join("one.txt"), "1\n");
    write(&nested.join("two.txt"), "2\n");
    let dirty = changes(&mut client, &workspace).await;
    assert_eq!(counts(&dirty), [1, 0, 0, 0, 0], "{dirty}");
    assert_ne!(dirty["token"], clean["token"]);

    // Committed: clean again, but the commit is on no remote.
    git(&nested, &["add", "."]);
    commit(&nested, "work");
    let committed = changes(&mut client, &workspace).await;
    assert_eq!(counts(&committed), [1, 0, 0, 0, 0], "{committed}");
    assert_ne!(committed["token"], dirty["token"]);

    // Pushed: nothing to lose again.
    git(
        &nested,
        &["push", "--quiet", "origin", "HEAD:refs/heads/work"],
    );
    let pushed = changes(&mut client, &workspace).await;
    assert_eq!(counts(&pushed), [0, 0, 0, 0, 0], "{pushed}");
}

// cites PROTO-CORE-API-R49 once planning PR 245 adds it
#[tokio::test(flavor = "multi_thread")]
async fn a_submodule_counts_when_it_holds_commits_its_remote_does_not() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let source = pushed_clone(core.dir.path(), "source");
    let allow_files = ["-c", "protocol.file.allow=always"];
    let source_path = source.display().to_string();
    git(
        &repo,
        &[
            &allow_files[..],
            &["submodule", "add", "--quiet", &source_path, "sub"],
        ]
        .concat(),
    );
    commit(&repo, "submodule");
    let (workspace, folder) = workspace_of(&mut client, &repo).await;

    // Not checked out yet: an empty folder holds nothing.
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 0, 0, 0]
    );
    git(
        &folder,
        &[
            &allow_files[..],
            &["submodule", "update", "--init", "--quiet"],
        ]
        .concat(),
    );
    let clean = changes(&mut client, &workspace).await;
    assert_eq!(counts(&clean), [0, 0, 0, 0, 0], "{clean}");

    // A commit on a branch of its own, then back on the recorded commit:
    // the workspace shows no change, yet the commit is held nowhere else.
    let sub = folder.join("sub");
    git(&sub, &["checkout", "--quiet", "-b", "work"]);
    commit(&sub, "work");
    git(&sub, &["checkout", "--quiet", "--detach", "HEAD~1"]);
    assert_eq!(git(&folder, &["status", "--porcelain"]), "");
    let held = changes(&mut client, &workspace).await;
    assert_eq!(counts(&held), [1, 0, 0, 0, 0], "{held}");
    assert_ne!(held["token"], clean["token"]);
}

// verifies: PROTO-CORE-API-R38
#[tokio::test(flavor = "multi_thread")]
async fn the_token_follows_tracked_files_content_and_the_index() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;

    // A tracked file edited, then edited again: the count stays, the
    // token follows the content.
    write(&folder.join("README.md"), "one\n");
    let first = changes(&mut client, &workspace).await;
    write(&folder.join("README.md"), "two\n");
    let second = changes(&mut client, &workspace).await;
    assert_eq!(counts(&first), [1, 0, 0, 0, 0], "{first}");
    assert_eq!(counts(&second), [1, 0, 0, 0, 0], "{second}");
    assert_ne!(second["token"], first["token"]);

    // Staged, then deleted from the folder: only the index holds it, and
    // staging other content changes nothing but the index.
    write(&folder.join("gone.txt"), "staged first\n");
    git(&folder, &["add", "gone.txt"]);
    std::fs::remove_file(folder.join("gone.txt")).unwrap();
    let staged = changes(&mut client, &workspace).await;
    write(&folder.join("gone.txt"), "staged second\n");
    git(&folder, &["add", "gone.txt"]);
    std::fs::remove_file(folder.join("gone.txt")).unwrap();
    let restaged = changes(&mut client, &workspace).await;
    assert_eq!(counts(&staged), [2, 0, 0, 0, 0], "{staged}");
    assert_eq!(counts(&restaged), [2, 0, 0, 0, 0], "{restaged}");
    assert_ne!(restaged["token"], staged["token"]);
}

// cites PROTO-CORE-API-R46 once planning PR 245 adds it
// cites PROTO-CORE-API-R49 once planning PR 245 adds it
#[tokio::test(flavor = "multi_thread")]
async fn a_submodule_whose_history_went_with_git_s_record_counts() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let source = pushed_clone(core.dir.path(), "source");
    let allow_files = ["-c", "protocol.file.allow=always"];
    let source_path = source.display().to_string();
    git(
        &repo,
        &[
            &allow_files[..],
            &["submodule", "add", "--quiet", &source_path, "sub"],
        ]
        .concat(),
    );
    commit(&repo, "submodule");
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    git(
        &folder,
        &[
            &allow_files[..],
            &["submodule", "update", "--init", "--quiet"],
        ]
        .concat(),
    );
    assert_eq!(
        counts(&changes(&mut client, &workspace).await),
        [0, 0, 0, 0, 0]
    );

    // A submodule of a worktree keeps its repository inside git's record
    // of the worktree: deleting the record leaves its files with no
    // history, which git lists as files like any other.
    let name = folder.file_name().unwrap();
    std::fs::remove_dir_all(repo.join(".git").join("worktrees").join(name)).unwrap();
    let found = changes(&mut client, &workspace).await;
    // README.md, .gitmodules and the submodule's README.md.
    assert_eq!(counts(&found), [3, 0, 0, 0, 0], "{found}");
}

// cites PROTO-CORE-API-R46 once planning PR 245 adds it
#[tokio::test(flavor = "multi_thread")]
async fn without_git_s_record_the_token_follows_the_files_left_and_the_branch() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    let branch = workspace["vcs_ref"].as_str().unwrap().to_string();
    write(&folder.join("a.txt"), "a\n");
    let recorded = changes(&mut client, &workspace).await;
    assert_eq!(recorded["unrecorded"], json!(false), "{recorded}");

    let name = folder.file_name().unwrap();
    std::fs::remove_dir_all(repo.join(".git").join("worktrees").join(name)).unwrap();
    let first = changes(&mut client, &workspace).await;
    assert_eq!(first["unrecorded"], json!(true), "{first}");
    assert_eq!(counts(&first), [2, 0, 0, 0, 0], "{first}");

    let mut seen = vec![first["token"].clone()];
    let mut expect_new = |found: &Value| {
        assert_eq!(found["unrecorded"], json!(true), "{found}");
        assert!(
            !seen.contains(&found["token"]),
            "the token did not change: {found}"
        );
        seen.push(found["token"].clone());
    };
    // A file left edited.
    write(&folder.join("a.txt"), "a, edited\n");
    let edited = changes(&mut client, &workspace).await;
    assert_eq!(counts(&edited), [2, 0, 0, 0, 0]);
    expect_new(&edited);
    // A file added.
    write(&folder.join("b.txt"), "b\n");
    let added = changes(&mut client, &workspace).await;
    assert_eq!(counts(&added), [3, 0, 0, 0, 0]);
    expect_new(&added);
    // A commit on the workspace's branch, made from the repository, since
    // the folder is no longer a checkout git knows.
    let tree = git(&repo, &["rev-parse", &format!("{branch}^{{tree}}")]);
    let commit = git(
        &repo,
        &[
            "commit-tree",
            "-p",
            &branch,
            "-m",
            "on the branch",
            tree.trim(),
        ],
    );
    git(
        &repo,
        &["update-ref", &format!("refs/heads/{branch}"), commit.trim()],
    );
    let committed = changes(&mut client, &workspace).await;
    assert_eq!(counts(&committed), [3, 0, 1, 0, 0]);
    expect_new(&committed);
}

// cites PROTO-CORE-API-R46 once planning PR 245 adds it
#[tokio::test(flavor = "multi_thread")]
async fn without_git_s_record_the_token_follows_ignored_entries_and_nested_repositories() {
    let (core, mut client) = setup().await;
    let repo = repo_with_ignores(core.dir.path());
    let (workspace, folder) = workspace_of(&mut client, &repo).await;
    let nested = folder.join("nested");
    git_repo(&nested);
    let name = folder.file_name().unwrap();
    std::fs::remove_dir_all(repo.join(".git").join("worktrees").join(name)).unwrap();
    let first = changes(&mut client, &workspace).await;
    assert_eq!(first["unrecorded"], json!(true), "{first}");
    // README.md, .gitignore, old.txt, and the nested repository, whose
    // commit is on no remote.
    assert_eq!(counts(&first), [4, 0, 0, 0, 0], "{first}");

    // An ignored file added.
    write(&folder.join("x.log"), "x\n");
    let ignored = changes(&mut client, &workspace).await;
    assert_eq!(counts(&ignored), [4, 1, 0, 0, 0], "{ignored}");
    assert_ne!(ignored["token"], first["token"]);

    // A new commit inside the nested repository: no count changes.
    commit(&nested, "more");
    let committed = changes(&mut client, &workspace).await;
    assert_eq!(counts(&committed), [4, 1, 0, 0, 0], "{committed}");
    assert_ne!(committed["token"], ignored["token"]);
}

// cites PROTO-CORE-API-R47 once planning PR 245 adds it
#[tokio::test(flavor = "multi_thread")]
async fn other_requests_on_the_connection_never_wait_for_it() {
    let (core, mut client) = setup().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let (workspace, _folder) = workspace_of(&mut client, &repo).await;
    // A slow `git status`: git asks the file system monitor the repository
    // names, run through the shell, before it answers. The worktree shares
    // the repository's configuration.
    git(&repo, &["config", "core.fsmonitor", "sleep 4 #"]);

    let started = std::time::Instant::now();
    let slow = client
        .request("workspace.changes", json!({ "id": workspace["id"] }))
        .await;
    let quick = client.request("session.list", json!({})).await;
    let listed = client.response(quick).await;
    assert!(listed.get("result").is_some(), "{listed:#}");
    // Nothing arrived ahead of session.list's answer: workspace.changes'
    // answer, had it come first, would be waiting here.
    assert_eq!(client.responses_waiting(), 0);
    let counted = client.response(slow).await;
    assert!(counted.get("result").is_some(), "{counted:#}");
    // The slow one really was slow.
    let slow_took = started.elapsed();
    assert!(slow_took >= Duration::from_secs(4), "{slow_took:?}");
}
