//! Projects and workspaces driven over the socket against temporary git
//! repositories.

mod support;

use std::path::Path;

use serde_json::{Value, json};
use support::deleting::{hold_the_turn, made_through, with_stopping_adds};
use support::{Client, TestCore, git, git_repo, git_repo_on, kind, normalized, start};

/// A started Core with a client, a git repository, and `workspaces.root`
/// pointed into the test's temporary directory.
async fn setup() -> (TestCore, Client, std::path::PathBuf) {
    let core = start().await;
    let mut client = core.client().await;
    let repo = core.dir.path().join("repo");
    git_repo(&repo);
    let root = core.dir.path().join("worktrees").display().to_string();
    client
        .result(
            "settings.set",
            json!({ "patch": { "workspaces": { "root": root } } }),
        )
        .await;
    (core, client, repo)
}

/// A `workspace.list` result without its root workspace, which comes first.
fn isolated(listed: &Value) -> Value {
    assert_eq!(listed[0]["kind"], json!("root"), "{listed}");
    Value::Array(listed.as_array().unwrap()[1..].to_vec())
}

async fn add_project(client: &mut Client, path: &Path) -> Value {
    client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": path.display().to_string(), "trusted": false }),
        )
        .await
}

// verifies: PROTO-CORE-API-R06
// verifies: PROTO-CORE-API-R20
#[tokio::test(flavor = "multi_thread")]
async fn project_lifecycle() {
    let (core, mut client, repo) = setup().await;

    let project = add_project(&mut client, &repo).await;
    assert_eq!(project["vcs"], json!("git"));
    assert_eq!(project["name"], json!("repo"));
    assert_eq!(project["host_id"], json!("local"));
    assert_eq!(project["path"], json!(normalized(&repo)));
    assert_eq!(project["trusted"], json!(false));

    // A trailing separator is the same directory, so the same project.
    let with_slash = format!("{}/", repo.display());
    let error = client
        .error(
            "project.add",
            json!({ "host_id": "local", "path": with_slash, "trusted": false }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state");
    assert_eq!(error["data"]["detail"], project["id"]);

    // A plain folder is a project without version control.
    let plain = core.dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let plain_project = add_project(&mut client, &plain).await;
    assert_eq!(plain_project["vcs"], json!("none"));

    let listed = client.result("project.list", json!({})).await;
    assert_eq!(listed, json!([project, plain_project]));

    let missing = core.dir.path().join("missing");
    let error = client
        .error(
            "project.add",
            json!({ "host_id": "local", "path": missing.display().to_string(), "trusted": false }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");

    let error = client
        .error(
            "project.add",
            json!({ "host_id": "local", "path": "relative/dir", "trusted": false }),
        )
        .await;
    assert_eq!(error["code"], json!(-32602));

    let error = client
        .error(
            "project.add",
            json!({ "host_id": "nowhere", "path": repo.display().to_string(), "trusted": false }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");

    let removed = client
        .result("project.remove", json!({ "id": project["id"] }))
        .await;
    assert_eq!(removed, project);
    let error = client
        .error("project.remove", json!({ "id": project["id"] }))
        .await;
    assert_eq!(kind(&error), "not_found");
    assert_eq!(
        client.result("project.list", json!({})).await,
        json!([plain_project])
    );
}

// verifies: PROTO-CORE-API-R11
#[tokio::test(flavor = "multi_thread")]
async fn a_project_is_trusted_only_when_said_and_stays_so_across_a_restart() {
    let (core, mut client, repo) = setup().await;
    let plain = core.dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();

    // No answer is no trust.
    let untrusted = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string() }),
        )
        .await;
    assert_eq!(untrusted["trusted"], json!(false));
    let trusted = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": plain.display().to_string(), "trusted": true }),
        )
        .await;
    assert_eq!(trusted["trusted"], json!(true));

    // `project.set_trust` changes it and returns the project as it now is.
    let changed = client
        .result(
            "project.set_trust",
            json!({ "id": untrusted["id"], "trusted": true }),
        )
        .await;
    assert_eq!(changed["trusted"], json!(true));
    assert_eq!(changed["id"], untrusted["id"]);
    let changed = client
        .result(
            "project.set_trust",
            json!({ "id": trusted["id"], "trusted": false }),
        )
        .await;
    assert_eq!(changed["trusted"], json!(false));
    let error = client
        .error(
            "project.set_trust",
            json!({ "id": "p-nowhere", "trusted": true }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");

    let before = client.result("project.list", json!({})).await;
    client.result("core.shutdown", json!({})).await;
    core.task.await.expect("joined").expect("stopped cleanly");

    let core = support::start_with(core.dir, |_| {}).await;
    let mut client = core.client().await;
    let after = client.result("project.list", json!({})).await;
    assert_eq!(after, before);
    let trust: Vec<&Value> = after
        .as_array()
        .unwrap()
        .iter()
        .map(|p| &p["trusted"])
        .collect();
    assert_eq!(trust, [&json!(true), &json!(false)]);
}

// verifies: PROTO-CORE-API-R06
// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn workspace_lifecycle() {
    let (core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let project_id = project["id"].clone();
    // A new project has its root workspace, its own folder, and nothing else.
    let listed = client
        .result("workspace.list", json!({ "project_id": project_id }))
        .await;
    let root = listed[0].clone();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(root["kind"], json!("root"));
    assert_eq!(root["path"], project["path"]);
    assert_eq!(root["state"], json!("ready"));

    // Default name: a branch Core picks, in a folder under the root.
    let first = client
        .result("workspace.create", json!({ "project_id": project_id }))
        .await;
    assert_eq!(first["state"], json!("ready"));
    assert_eq!(first["kind"], json!("isolated"));
    let id = first["id"].as_str().unwrap();
    assert_eq!(first["vcs_ref"], json!(format!("ersa/{id}")));
    let first_path = Path::new(first["path"].as_str().unwrap()).to_path_buf();
    assert!(
        first_path.starts_with(core.dir.path().join("worktrees")),
        "outside the project tree, under workspaces.root: {}",
        first_path.display()
    );
    assert!(first_path.join("README.md").is_file(), "checked out");
    assert_eq!(
        git(&first_path, &["branch", "--show-current"]).trim(),
        format!("ersa/{id}")
    );

    // A chosen branch name and base.
    git(
        &repo,
        &["commit", "--quiet", "--allow-empty", "-m", "second"],
    );
    let named = client
        .result(
            "workspace.create",
            json!({ "project_id": project_id, "name": "feature/rail", "base": "main~1" }),
        )
        .await;
    assert_eq!(named["vcs_ref"], json!("feature/rail"));
    let named_path = Path::new(named["path"].as_str().unwrap()).to_path_buf();
    assert_eq!(
        git(&named_path, &["rev-parse", "HEAD"]),
        git(&repo, &["rev-parse", "main~1"])
    );

    let listed = client
        .result("workspace.list", json!({ "project_id": project_id }))
        .await;
    // The root workspace first.
    assert_eq!(listed, json!([root, first, named]));

    // The same branch twice is git's refusal, reported as a VCS error, and
    // leaves no workspace behind.
    let error = client
        .error(
            "workspace.create",
            json!({ "project_id": project_id, "name": "feature/rail" }),
        )
        .await;
    assert_eq!(kind(&error), "vcs_error");
    assert!(error["data"]["detail"].is_string());
    let listed = client
        .result("workspace.list", json!({ "project_id": project_id }))
        .await;
    assert_eq!(listed.as_array().unwrap().len(), 3);

    // The root workspace is removed only with its project.
    let error = client
        .error("workspace.remove", json!({ "id": root["id"] }))
        .await;
    assert_eq!(kind(&error), "invalid_state");

    // A project with isolated workspaces cannot be removed.
    let error = client
        .error("project.remove", json!({ "id": project_id }))
        .await;
    assert_eq!(kind(&error), "invalid_state");

    // Removal deletes the folder and keeps the branch.
    let removed = client
        .result("workspace.remove", json!({ "id": first["id"] }))
        .await;
    assert_eq!(removed["id"], first["id"]);
    assert!(!first_path.exists(), "the worktree folder is gone");
    assert!(git(&repo, &["branch", "--list", &format!("ersa/{id}")]).contains(id));

    // Local changes block removal without force, and the workspace stays ready.
    std::fs::write(named_path.join("notes.txt"), "uncommitted").unwrap();
    let error = client
        .error("workspace.remove", json!({ "id": named["id"] }))
        .await;
    assert_eq!(kind(&error), "vcs_error");
    let listed = client
        .result("workspace.list", json!({ "project_id": project_id }))
        .await;
    assert_eq!(listed, json!([root, named]));
    client
        .result(
            "workspace.remove",
            json!({ "id": named["id"], "force": true }),
        )
        .await;
    assert!(!named_path.exists());

    let error = client
        .error("workspace.remove", json!({ "id": named["id"] }))
        .await;
    assert_eq!(kind(&error), "not_found");

    // With no isolated workspaces left, the project can go, and its root
    // workspace with it, leaving its folder as it was.
    client
        .result("project.remove", json!({ "id": project_id }))
        .await;
    let error = client
        .error("workspace.list", json!({ "project_id": project_id }))
        .await;
    assert_eq!(kind(&error), "not_found");
    assert!(repo.join("README.md").is_file());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worktree_deleted_by_hand_is_still_removable() {
    let (_core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    std::fs::remove_dir_all(workspace["path"].as_str().unwrap()).unwrap();

    client
        .result("workspace.remove", json!({ "id": workspace["id"] }))
        .await;
    let worktrees = git(&repo, &["worktree", "list", "--porcelain"]);
    assert!(
        !worktrees.contains(workspace["id"].as_str().unwrap()),
        "git forgot the worktree: {worktrees}"
    );
}

/// A project whose version control has no workspace adapter has its root
/// workspace, and no isolated one.
// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn workspaces_need_a_git_project_and_safe_names() {
    let (core, mut client, repo) = setup().await;
    let plain = core.dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let plain_project = add_project(&mut client, &plain).await;
    let error = client
        .error(
            "workspace.create",
            json!({ "project_id": plain_project["id"] }),
        )
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let listed = client
        .result(
            "workspace.list",
            json!({ "project_id": plain_project["id"] }),
        )
        .await;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["kind"], json!("root"));
    assert_eq!(listed[0]["path"], plain_project["path"]);

    let project = add_project(&mut client, &repo).await;
    for params in [
        json!({ "project_id": project["id"], "name": "--orphan" }),
        json!({ "project_id": project["id"], "base": "-q" }),
    ] {
        let error = client.error("workspace.create", params).await;
        assert_eq!(error["code"], json!(-32602));
    }

    let error = client
        .error("workspace.create", json!({ "project_id": "p-nothing" }))
        .await;
    assert_eq!(kind(&error), "not_found");
    let error = client
        .error("workspace.list", json!({ "project_id": "p-nothing" }))
        .await;
    assert_eq!(kind(&error), "not_found");
}

// verifies: PROTO-CORE-API-R20
#[tokio::test(flavor = "multi_thread")]
async fn project_paths_are_canonical_and_unique_however_spelled() {
    let (core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let stored = project["path"].as_str().unwrap().to_string();
    assert!(
        !stored.starts_with(r"\?\"),
        "not the verbatim form: {stored}"
    );
    assert!(Path::new(&stored).is_absolute());

    // The same folder through `..`, and (where the file system ignores case)
    // in other letter case, is the same project.
    let mut spellings = vec![repo.join("..").join("repo").display().to_string()];
    if cfg!(windows) {
        spellings.push(repo.display().to_string().to_uppercase());
    }
    for spelling in spellings {
        let error = client
            .error(
                "project.add",
                json!({ "host_id": "local", "path": spelling, "trusted": false }),
            )
            .await;
        assert_eq!(kind(&error), "invalid_state", "{spelling}");
        assert_eq!(error["data"]["detail"], project["id"]);
    }
    // Refused, so nothing was added.
    assert_eq!(
        client.result("project.list", json!({})).await,
        json!([project])
    );

    // A drive or file system root has no folder name to file workspaces
    // under, so it cannot be a project.
    let root = core
        .dir
        .path()
        .ancestors()
        .last()
        .unwrap()
        .display()
        .to_string();
    let error = client
        .error(
            "project.add",
            json!({ "host_id": "local", "path": root, "trusted": false }),
        )
        .await;
    assert_eq!(error["code"], json!(-32602), "{root}: {error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_locked_worktree_is_refused_even_with_force() {
    let (_core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let path = workspace["path"].as_str().unwrap();
    git(
        &repo,
        &["worktree", "lock", "--reason", "kept for review", path],
    );

    for params in [
        json!({ "id": workspace["id"] }),
        json!({ "id": workspace["id"], "force": true }),
    ] {
        let error = client.error("workspace.remove", params).await;
        assert_eq!(kind(&error), "vcs_error");
        let message = error["message"].as_str().unwrap();
        assert!(
            message.contains("locked") && message.contains("kept for review"),
            "{message}"
        );
    }
    // A refusal leaves the workspace as it was.
    let listed = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(isolated(&listed), json!([workspace]));
    assert!(Path::new(path).exists());

    git(&repo, &["worktree", "unlock", path]);
    client
        .result("workspace.remove", json!({ "id": workspace["id"] }))
        .await;
    assert!(!Path::new(path).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_workspaces_become_error_at_startup() {
    let (core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let creating = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let removing = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    client.result("core.shutdown", json!({})).await;
    core.task.await.expect("joined").expect("stopped cleanly");

    // Leave the rows as a Core that died mid-operation would have.
    let db = core.dir.path().join("data").join("ersa.db");
    let store = ersa_core::persistence::Store::open(&db).expect("open");
    let ids = (
        creating["id"].as_str().unwrap().to_string(),
        removing["id"].as_str().unwrap().to_string(),
    );
    store
        .call(move |conn| {
            conn.execute(
                "UPDATE workspaces SET state = 'creating' WHERE id = ?1",
                [&ids.0],
            )?;
            conn.execute(
                "UPDATE workspaces SET state = 'removing' WHERE id = ?1",
                [&ids.1],
            )?;
            Ok(())
        })
        .await
        .expect("update");
    drop(store);

    let core = support::start_with(core.dir, |_| {}).await;
    let mut client = core.client().await;
    let listed = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    let listed = isolated(&listed);
    let states: Vec<&Value> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|w| &w["state"])
        .collect();
    assert_eq!(states, [&json!("error"), &json!("error")]);

    // `error` workspaces can be removed.
    for workspace in listed.as_array().unwrap() {
        client
            .result("workspace.remove", json!({ "id": workspace["id"] }))
            .await;
    }
}

/// The worktree's administrative folder inside the repository
/// (`.git/worktrees/<name>`), where git keeps its lock file.
fn admin_dir(worktree: &Path) -> std::path::PathBuf {
    let git_dir = git(worktree, &["rev-parse", "--absolute-git-dir"]);
    std::path::PathBuf::from(git_dir.trim())
}

/// Sets a workspace's row state directly, as a Core that died would leave
/// it, in the database of the stopped Core whose folder is `dir`.
async fn set_state(dir: &Path, id: &Value, state: &'static str) {
    let db = dir.join("data").join("ersa.db");
    let store = ersa_core::persistence::Store::open(&db).expect("open");
    let id = id.as_str().unwrap().to_string();
    store
        .call(move |conn| {
            conn.execute(
                "UPDATE workspaces SET state = ?2 WHERE id = ?1",
                [id.as_str(), state],
            )?;
            Ok(())
        })
        .await
        .expect("update");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_killed_add_leaves_a_lock_that_only_an_error_workspace_overrides() {
    let (core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let path = Path::new(workspace["path"].as_str().unwrap()).to_path_buf();
    // What `git worktree add` leaves when it is killed before finishing.
    std::fs::write(admin_dir(&path).join("locked"), "initializing").unwrap();

    // On a workspace that finished, that lock is a lock like any other.
    let error = client
        .error(
            "workspace.remove",
            json!({ "id": workspace["id"], "force": true }),
        )
        .await;
    assert_eq!(kind(&error), "vcs_error");
    assert!(path.exists());

    // On one that never finished, it is the add's leftover: a Core that
    // died making it left the row `creating`, which the next start marks
    // `error`.
    client.result("core.shutdown", json!({})).await;
    core.task.await.expect("joined").expect("stopped cleanly");
    set_state(core.dir.path(), &workspace["id"], "creating").await;
    let core = support::start_with(core.dir, |_| {}).await;
    let mut client = core.client().await;
    client
        .result("workspace.remove", json!({ "id": workspace["id"] }))
        .await;
    assert!(!path.exists(), "the half-made worktree is gone");
    let listed = git(&repo, &["worktree", "list", "--porcelain"]);
    assert!(
        !listed.contains(workspace["id"].as_str().unwrap()),
        "{listed}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn discarding_overrides_an_initializing_lock() {
    let (_core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let path = workspace["path"].as_str().unwrap();
    std::fs::write(admin_dir(Path::new(path)).join("locked"), "initializing").unwrap();

    let repo_text = repo.display().to_string();
    ersa_core::workspace::git_worktree_discard(&ersa_adapter::LocalTransport, &repo_text, path)
        .await
        .expect("discarded");
    assert!(!Path::new(path).exists());
}

/// Makes `link` lead to `target`: a junction on Windows, a symbolic link
/// elsewhere.
fn link_dir(target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("mklink runs");
        assert!(status.status.success(), "{status:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_workspaces_root_behind_a_link_still_removes_cleanly() {
    let (core, mut client, repo) = setup().await;
    let real = core.dir.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    let link = core.dir.path().join("link");
    link_dir(&real, &link);
    let root = link.join("worktrees").display().to_string();
    client
        .result(
            "settings.set",
            json!({ "patch": { "workspaces": { "root": root } } }),
        )
        .await;

    let project = add_project(&mut client, &repo).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let path = Path::new(workspace["path"].as_str().unwrap()).to_path_buf();
    let canonical_real = ersa_core::platform::canonicalize(&real).unwrap();
    assert!(
        path.starts_with(&canonical_real),
        "stored through the real folder, as git reports it: {}",
        path.display()
    );

    client
        .result("workspace.remove", json!({ "id": workspace["id"] }))
        .await;
    assert!(
        !path.exists(),
        "removed from disk, not just from the database"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_folder_git_does_not_list_is_refused_not_reported_removed() {
    let (_core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    let path = Path::new(workspace["path"].as_str().unwrap()).to_path_buf();
    // git forgets the worktree; its folder stays.
    std::fs::remove_dir_all(admin_dir(&path)).unwrap();

    let error = client
        .error("workspace.remove", json!({ "id": workspace["id"] }))
        .await;
    assert_eq!(kind(&error), "vcs_error");
    assert!(error["message"].as_str().unwrap().contains("does not list"));
    assert!(path.exists());
    let listed = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(
        isolated(&listed),
        json!([workspace]),
        "kept, in the state it had"
    );
}

// -- The default branch a workspace starts from ------------------------------

/// A clone, at `dir/clone`, of an upstream whose HEAD is `develop`, a commit
/// ahead of `main`, so the clone's `origin/HEAD` points at `origin/develop`.
/// The clone has `develop` checked out.
fn clone_of_develop_upstream(dir: &Path) -> std::path::PathBuf {
    let upstream = dir.join("upstream");
    git_repo_on(&upstream, "main");
    git(&upstream, &["switch", "--quiet", "-c", "develop"]);
    git(
        &upstream,
        &["commit", "--quiet", "--allow-empty", "-m", "on develop"],
    );
    let clone = dir.join("clone");
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

/// Commits once on a new branch `name` at `repo`, leaving it checked out, so
/// it is a commit away from every other branch.
fn switch_to_new_branch_with_a_commit(repo: &Path, name: &str) {
    git(repo, &["switch", "--quiet", "-c", name]);
    git(
        repo,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            &format!("on {name}"),
        ],
    );
}

/// The commit a workspace created without a `base` in the project at `path`
/// starts from.
async fn default_start(client: &mut Client, path: &Path) -> String {
    let project = add_project(client, path).await;
    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    head_of(&workspace)
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn the_default_branch_is_the_one_origin_head_points_at() {
    let (core, mut client, _) = setup().await;
    let clone = clone_of_develop_upstream(core.dir.path());
    git(&clone, &["branch", "main", "origin/main"]);
    // Checked out elsewhere, and `main` exists: still `develop`.
    switch_to_new_branch_with_a_commit(&clone, "work");

    let start = default_start(&mut client, &clone).await;
    assert_eq!(start, git(&clone, &["rev-parse", "develop"]));
    assert_ne!(start, git(&clone, &["rev-parse", "main"]));
    assert_ne!(start, git(&clone, &["rev-parse", "work"]));
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn without_a_local_branch_for_origin_head_main_is_the_default() {
    let (core, mut client, _) = setup().await;
    let clone = clone_of_develop_upstream(core.dir.path());
    // `origin/HEAD` still points at `origin/develop`, which has no local
    // branch any more.
    git(&clone, &["switch", "--quiet", "-c", "main", "origin/main"]);
    git(&clone, &["branch", "--quiet", "-D", "develop"]);
    switch_to_new_branch_with_a_commit(&clone, "aaa");

    let start = default_start(&mut client, &clone).await;
    assert_eq!(start, git(&clone, &["rev-parse", "main"]));
    assert_ne!(start, git(&clone, &["rev-parse", "origin/develop"]));
    assert_ne!(start, git(&clone, &["rev-parse", "aaa"]));
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn with_only_master_master_is_the_default_whatever_is_checked_out() {
    let (core, mut client, _) = setup().await;
    let repo = core.dir.path().join("old");
    git_repo_on(&repo, "master");
    switch_to_new_branch_with_a_commit(&repo, "alpha");

    let start = default_start(&mut client, &repo).await;
    assert_eq!(start, git(&repo, &["rev-parse", "master"]));
    assert_ne!(start, git(&repo, &["rev-parse", "alpha"]));
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn with_none_of_them_the_checked_out_branch_is_the_default() {
    let (core, mut client, _) = setup().await;
    let repo = core.dir.path().join("trunk");
    git_repo_on(&repo, "trunk");
    switch_to_new_branch_with_a_commit(&repo, "topic");

    let start = default_start(&mut client, &repo).await;
    assert_eq!(start, git(&repo, &["rev-parse", "topic"]));
    assert_ne!(start, git(&repo, &["rev-parse", "trunk"]));
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn a_repository_broken_after_it_was_added_is_gits_failure() {
    let (core, mut client, _) = setup().await;
    // A folder that stopped being a repository after it was added: git's
    // own failure, with its message, not the caller's mistake, with a base
    // or without one.
    let gone = core.dir.path().join("gone");
    git_repo_on(&gone, "main");
    let project = add_project(&mut client, &gone).await;
    std::fs::remove_dir_all(gone.join(".git")).unwrap();
    std::fs::write(
        gone.join(".git"),
        "gitdir: nowhere
",
    )
    .unwrap();
    for params in [
        json!({ "project_id": project["id"], "base": "main" }),
        json!({ "project_id": project["id"] }),
    ] {
        let error = client.error("workspace.create", params.clone()).await;
        assert_eq!(kind(&error), "vcs_error", "{params}: {error}");
        assert!(error["data"]["detail"].is_string(), "{params}: {error}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn branch_list_is_not_a_method() {
    let (_core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let error = client
        .error("branch.list", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(error["code"], json!(-32601), "{error}");
}

// -- A workspace's base -------------------------------------------------------

/// The commit the workspace's checkout is at.
fn head_of(workspace: &Value) -> String {
    git(
        Path::new(workspace["path"].as_str().unwrap()),
        &["rev-parse", "HEAD"],
    )
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn a_base_is_any_revision_git_resolves_and_nothing_else() {
    let (_core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let first = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["tag", "-a", "-m", "v1", "v1"]);
    git(
        &repo,
        &["commit", "--quiet", "--allow-empty", "-m", "second"],
    );
    git(&repo, &["branch", "feature"]);

    for (base, expected) in [
        ("feature", git(&repo, &["rev-parse", "feature"])),
        // An annotated tag starts from the commit it tags.
        ("v1", first.clone()),
        (first.trim(), first.clone()),
    ] {
        let workspace = client
            .result(
                "workspace.create",
                json!({ "project_id": project["id"], "base": base }),
            )
            .await;
        assert_eq!(head_of(&workspace), expected, "{base}");
    }

    // Something git cannot resolve is the caller's mistake, and leaves no
    // workspace or branch behind.
    let before = client
        .result("workspace.list", json!({ "project_id": project["id"] }))
        .await;
    // `main@{50}` is past the end of `main`'s reflog, which some git
    // versions report with the exit code of a broken repository.
    for base in ["no-such-branch", "main~99", "HEAD:README.md", "main@{50}"] {
        let error = client
            .error(
                "workspace.create",
                json!({ "project_id": project["id"], "name": "never", "base": base }),
            )
            .await;
        assert_eq!(error["code"], json!(-32602), "{base}: {error}");
    }
    assert_eq!(
        client
            .result("workspace.list", json!({ "project_id": project["id"] }))
            .await,
        before
    );
    assert_eq!(git(&repo, &["branch", "--list", "never"]), "");
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn without_a_base_a_workspace_starts_from_the_default_branch() {
    let (_core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    // The project's folder has another branch checked out, a commit ahead.
    git(&repo, &["switch", "--quiet", "-c", "side"]);
    git(
        &repo,
        &["commit", "--quiet", "--allow-empty", "-m", "on side"],
    );

    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(head_of(&workspace), git(&repo, &["rev-parse", "main"]));
    assert_ne!(head_of(&workspace), git(&repo, &["rev-parse", "side"]));
}

// verifies: PROTO-CORE-API-R19
#[tokio::test(flavor = "multi_thread")]
async fn without_a_base_or_a_default_a_workspace_starts_from_the_checked_out_commit() {
    let (core, mut client, _) = setup().await;
    let repo = core.dir.path().join("trunk");
    git_repo_on(&repo, "trunk");
    git(&repo, &["switch", "--quiet", "--detach"]);
    git(
        &repo,
        &["commit", "--quiet", "--allow-empty", "-m", "detached"],
    );
    let project = add_project(&mut client, &repo).await;

    let workspace = client
        .result("workspace.create", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(head_of(&workspace), git(&repo, &["rev-parse", "HEAD"]));
    assert_ne!(head_of(&workspace), git(&repo, &["rev-parse", "trunk"]));
}

// -- Finding a project by its folder -------------------------------------------

// verifies: PROTO-CORE-API-R27
#[tokio::test(flavor = "multi_thread")]
async fn project_find_matches_a_folder_as_project_add_compares_them() {
    let (core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;

    let mut spellings = vec![
        repo.display().to_string(),
        format!("{}/", repo.display()),
        repo.join("..").join("repo").display().to_string(),
    ];
    if cfg!(windows) {
        spellings.push(repo.display().to_string().to_uppercase());
    }
    for spelling in spellings {
        let found = client
            .result(
                "project.find",
                json!({ "host_id": "local", "path": spelling }),
            )
            .await;
        assert_eq!(found, project, "{spelling}");
    }

    // A folder that is not a project, or not there at all, is none.
    let plain = core.dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    for path in [plain.clone(), core.dir.path().join("missing")] {
        let found = client
            .result(
                "project.find",
                json!({ "host_id": "local", "path": path.display().to_string() }),
            )
            .await;
        assert_eq!(found, Value::Null, "{}", path.display());
    }

    // A project whose folder has gone is not found there: `project.add`
    // would refuse that folder too.
    let gone = core.dir.path().join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    add_project(&mut client, &gone).await;
    std::fs::remove_dir_all(&gone).unwrap();
    let found = client
        .result(
            "project.find",
            json!({ "host_id": "local", "path": gone.display().to_string() }),
        )
        .await;
    assert_eq!(found, Value::Null);

    let error = client
        .error(
            "project.find",
            json!({ "host_id": "local", "path": "relative/dir" }),
        )
        .await;
    assert_eq!(error["code"], json!(-32602));
    let error = client
        .error(
            "project.find",
            json!({ "host_id": "nowhere", "path": repo.display().to_string() }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");
}

// git dies when two of its worktree commands run in one repository at once
// (each reads the others' records while one is half written), so a second
// `workspace.create` in a project waits for the first one's add, then
// makes its own: both are made.
// verifies: PROTO-CORE-API-R118
#[tokio::test(flavor = "multi_thread")]
async fn a_second_workspace_made_at_once_waits_for_the_firsts_git() {
    use std::sync::Arc;
    use std::time::Duration;

    let gate = Arc::new(ersa_core::Gate::default());
    let wait = ersa_core::workspace::WORKTREE_TURN_WAIT;
    let (core, _git, _client, project) = with_stopping_adds(&gate, wait, Vec::new()).await;
    let params = json!({ "project_id": project["id"] });

    let mut first = core.client().await;
    let made_first = first.request("workspace.create", params.clone()).await;
    let reached = tokio::time::timeout(Duration::from_secs(30), gate.reached()).await;
    reached.expect("the first add began");
    // The second waits its turn: its git does not begin beside the first's.
    let mut second = core.client().await;
    let made_second = second.request("workspace.create", params).await;
    let began = tokio::time::timeout(Duration::from_millis(500), gate.reached()).await;
    assert!(began.is_err(), "a second add began beside the first");

    // The first's add goes on and ends; then the second's begins.
    gate.go();
    let made = first.response(made_first).await;
    assert_eq!(made["result"]["state"], json!("ready"), "{made}");
    let reached = tokio::time::timeout(Duration::from_secs(30), gate.reached()).await;
    reached.expect("the second add began once the first ended");
    gate.go();
    let made = second.response(made_second).await;
    assert_eq!(made["result"]["state"], json!("ready"), "{made}");
}

// A request waits 300 seconds for its turn at a project's worktrees.
// verifies: PROTO-CORE-API-R119
#[test]
fn a_turn_at_the_worktrees_is_waited_for_300_seconds() {
    let wait = std::time::Duration::from_secs(300);
    assert_eq!(ersa_core::workspace::WORKTREE_TURN_WAIT, wait);
    let config = ersa_core::CoreConfig::new(std::path::PathBuf::from("data")).unwrap();
    assert_eq!(config.worktree_turn_wait, wait);
}

// A `workspace.create` that waited its whole limit for another change to
// the project's worktrees starts no git work: it answers `vcs_error` and
// makes no workspace. A `workspace.remove` likewise, leaving the workspace
// as it was.
// verifies: PROTO-CORE-API-R119
#[tokio::test(flavor = "multi_thread")]
async fn a_change_that_waited_too_long_for_its_turn_runs_no_git_and_answers_vcs_error() {
    use std::sync::Arc;
    use std::time::Duration;

    let gate = Arc::new(ersa_core::Gate::default());
    let wait = Duration::from_millis(300);
    let (core, git, mut client, project) = with_stopping_adds(&gate, wait, Vec::new()).await;
    let params = json!({ "project_id": project["id"] });

    // A workspace to remove, made first.
    let kept = made_through(&gate, &mut client, &project).await;

    // A create whose add stands stopped holds the project's turn.
    let mut holder = core.client().await;
    let holding = hold_the_turn(&gate, &mut holder, &project).await;
    let before = git.changes();

    // A second create waits its limit, then answers without running git.
    let refused = client.call("workspace.create", params.clone()).await;
    assert_eq!(kind(&refused["error"]), "vcs_error", "{refused}");
    let listed = client.result("workspace.list", params.clone()).await;
    assert_eq!(isolated(&listed).as_array().unwrap().len(), 2, "{listed}");

    // A removal likewise: the workspace stays, ready, with its folder.
    let remove = json!({ "id": kept["id"] });
    let refused = client.call("workspace.remove", remove).await;
    assert_eq!(kind(&refused["error"]), "vcs_error", "{refused}");
    let listed = client.result("workspace.list", params).await;
    let still = isolated(&listed)
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["id"] == kept["id"])
        .cloned()
        .expect("the workspace is still listed");
    assert_eq!(still["state"], json!("ready"), "{still}");
    assert!(Path::new(kept["path"].as_str().unwrap()).is_dir());

    // Neither added, removed nor pruned a worktree.
    assert_eq!(git.changes(), before);
    let began = tokio::time::timeout(Duration::from_millis(500), gate.reached()).await;
    assert!(began.is_err(), "an add began for the create that gave up");

    // The holder goes on unharmed.
    gate.go();
    let made = holder.response(holding).await;
    assert_eq!(made["result"]["state"], json!("ready"), "{made}");
}
