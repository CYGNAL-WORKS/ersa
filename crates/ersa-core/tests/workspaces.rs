//! Projects and workspaces driven over the socket against temporary git
//! repositories.

mod support;

use std::path::Path;

use serde_json::{Value, json};
use support::{Client, TestCore, git, git_repo, kind, normalized, start};

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

async fn add_project(client: &mut Client, path: &Path) -> Value {
    client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": path.display().to_string() }),
        )
        .await
}

// verifies: PROTO-CORE-API-R06
#[tokio::test(flavor = "multi_thread")]
async fn project_lifecycle() {
    let (core, mut client, repo) = setup().await;

    let project = add_project(&mut client, &repo).await;
    assert_eq!(project["vcs"], json!("git"));
    assert_eq!(project["name"], json!("repo"));
    assert_eq!(project["host_id"], json!("local"));
    assert_eq!(project["path"], json!(normalized(&repo)));

    // A trailing separator is the same directory, so the same project.
    let with_slash = format!("{}/", repo.display());
    let error = client
        .error(
            "project.add",
            json!({ "host_id": "local", "path": with_slash }),
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
            json!({ "host_id": "local", "path": missing.display().to_string() }),
        )
        .await;
    assert_eq!(kind(&error), "not_found");

    let error = client
        .error(
            "project.add",
            json!({ "host_id": "local", "path": "relative/dir" }),
        )
        .await;
    assert_eq!(error["code"], json!(-32602));

    let error = client
        .error(
            "project.add",
            json!({ "host_id": "nowhere", "path": repo.display().to_string() }),
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

// verifies: PROTO-CORE-API-R06
#[tokio::test(flavor = "multi_thread")]
async fn workspace_lifecycle() {
    let (core, mut client, repo) = setup().await;
    let project = add_project(&mut client, &repo).await;
    let project_id = project["id"].clone();
    assert_eq!(
        client
            .result("workspace.list", json!({ "project_id": project_id }))
            .await,
        json!([])
    );

    // Default name: a branch Core picks, in a folder under the root.
    let first = client
        .result("workspace.create", json!({ "project_id": project_id }))
        .await;
    assert_eq!(first["state"], json!("ready"));
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
    assert_eq!(listed, json!([first, named]));

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
    assert_eq!(listed.as_array().unwrap().len(), 2);

    // A project with workspaces cannot be removed.
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
    assert_eq!(listed, json!([named]));
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

    // With no workspaces left, the project can go.
    client
        .result("project.remove", json!({ "id": project_id }))
        .await;
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
