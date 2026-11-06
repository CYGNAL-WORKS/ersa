//! Projects and workspaces driven over the socket against temporary git
//! repositories.

mod support;

use std::path::Path;

use serde_json::{Value, json};
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

/// Sets a workspace's row state directly, as a Core that died would leave it.
async fn set_state(core: &TestCore, id: &Value, state: &'static str) {
    let db = core.dir.path().join("data").join("ersa.db");
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

    // On one that never finished (`error`), it is the add's leftover.
    set_state(&core, &workspace["id"], "error").await;
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

// -- Branches for a base ------------------------------------------------------

/// A clone, at `dir/clone`, of an upstream whose HEAD is `develop`, so the
/// clone's `origin/HEAD` points at `origin/develop`. The clone has `develop`
/// checked out.
fn clone_of_develop_upstream(dir: &Path) -> std::path::PathBuf {
    let upstream = dir.join("upstream");
    git_repo_on(&upstream, "main");
    git(&upstream, &["branch", "develop"]);
    git(&upstream, &["symbolic-ref", "HEAD", "refs/heads/develop"]);
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

/// `branch.list` for the project at `path`, as `(name, default)` pairs.
async fn branches(client: &mut Client, path: &Path) -> Vec<(String, bool)> {
    let project = add_project(client, path).await;
    let listed = client
        .result("branch.list", json!({ "project_id": project["id"] }))
        .await;
    listed
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["name"].as_str().unwrap().to_string(),
                b["default"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn pairs(expected: &[(&str, bool)]) -> Vec<(String, bool)> {
    expected.iter().map(|(n, d)| (n.to_string(), *d)).collect()
}

// verifies: PROTO-CORE-API-R18
#[tokio::test(flavor = "multi_thread")]
async fn the_default_branch_is_the_one_origin_head_points_at() {
    let (core, mut client, _) = setup().await;
    let clone = clone_of_develop_upstream(core.dir.path());

    git(&clone, &["branch", "main", "origin/main"]);
    // Checked out elsewhere, and `main` exists: still `develop`.
    git(&clone, &["switch", "--quiet", "-c", "work"]);

    assert_eq!(
        branches(&mut client, &clone).await,
        pairs(&[("develop", true), ("main", false), ("work", false)]),
        "local branches only, sorted by name"
    );
}

// verifies: PROTO-CORE-API-R18
#[tokio::test(flavor = "multi_thread")]
async fn without_a_local_branch_for_origin_head_main_is_the_default() {
    let (core, mut client, _) = setup().await;
    let clone = clone_of_develop_upstream(core.dir.path());

    // `origin/HEAD` still points at `origin/develop`, which has no local
    // branch any more.
    git(&clone, &["switch", "--quiet", "-c", "main", "origin/main"]);
    git(&clone, &["branch", "--quiet", "-D", "develop"]);
    git(&clone, &["switch", "--quiet", "-c", "aaa"]);

    assert_eq!(
        branches(&mut client, &clone).await,
        pairs(&[("aaa", false), ("main", true)])
    );
}

// verifies: PROTO-CORE-API-R18
#[tokio::test(flavor = "multi_thread")]
async fn with_only_master_master_is_the_default_whatever_is_checked_out() {
    let (core, mut client, _) = setup().await;
    let repo = core.dir.path().join("old");
    git_repo_on(&repo, "master");
    git(&repo, &["switch", "--quiet", "-c", "alpha"]);

    assert_eq!(
        branches(&mut client, &repo).await,
        pairs(&[("alpha", false), ("master", true)])
    );
}

// verifies: PROTO-CORE-API-R18
#[tokio::test(flavor = "multi_thread")]
async fn with_none_of_them_the_checked_out_branch_is_the_default() {
    let (core, mut client, _) = setup().await;
    let repo = core.dir.path().join("trunk");
    git_repo_on(&repo, "trunk");
    git(&repo, &["switch", "--quiet", "-c", "topic"]);

    assert_eq!(
        branches(&mut client, &repo).await,
        pairs(&[("topic", true), ("trunk", false)])
    );
}

// verifies: PROTO-CORE-API-R18
#[tokio::test(flavor = "multi_thread")]
async fn a_detached_checkout_with_none_of_them_has_no_default() {
    let (core, mut client, _) = setup().await;
    let repo = core.dir.path().join("trunk");
    git_repo_on(&repo, "trunk");
    git(&repo, &["switch", "--quiet", "--detach"]);

    assert_eq!(
        branches(&mut client, &repo).await,
        pairs(&[("trunk", false)])
    );
}

// verifies: PROTO-CORE-API-R18
#[tokio::test(flavor = "multi_thread")]
async fn branch_list_needs_a_git_project() {
    let (core, mut client, _) = setup().await;
    let plain = core.dir.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let project = add_project(&mut client, &plain).await;
    let error = client
        .error("branch.list", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(kind(&error), "invalid_state");
    let error = client
        .error("branch.list", json!({ "project_id": "p-nothing" }))
        .await;
    assert_eq!(kind(&error), "not_found");

    // A folder that stopped being a repository after it was added: git's
    // own failure, with its message.
    let gone = core.dir.path().join("gone");
    git_repo_on(&gone, "main");
    let project = add_project(&mut client, &gone).await;
    std::fs::remove_dir_all(gone.join(".git")).unwrap();
    std::fs::write(gone.join(".git"), "gitdir: nowhere\n").unwrap();
    let error = client
        .error("branch.list", json!({ "project_id": project["id"] }))
        .await;
    assert_eq!(kind(&error), "vcs_error");
    assert!(error["data"]["detail"].is_string(), "{error}");
    // A base can't be resolved there either, and that is git's failure, not
    // the caller's mistake.
    let error = client
        .error(
            "workspace.create",
            json!({ "project_id": project["id"], "base": "main" }),
        )
        .await;
    assert_eq!(kind(&error), "vcs_error", "{error}");
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
