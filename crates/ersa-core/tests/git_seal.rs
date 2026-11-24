//! Tests' git is sealed from the configuration of whoever runs them
//! (ARCH-CORE, Verification): a global hook or commit signing in the user's
//! home reaches neither a test's own git nor the git of a Core a test starts.
//!
//! A hostile home ([`sealed::hostile_home`]) fails every commit and every
//! worktree. The first test shows it bites git that is not sealed; the last
//! runs an ordinary test, which commits through the test support and has its
//! Core make a worktree, in a child process that inherits that home.

mod support;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use ersa_adapter::{Process, ProcessSpec, Transport};
use ersa_core::transport::LocalTreeTransport;
use ersa_core::{CommandTransport, Server};
use serde_json::{Value, json};
use support::{Client, git_repo, kind, sealed, start, tempdir, test_config, unique_endpoint};

/// Core's command transport with `env` added to every process it starts, as
/// if Core had inherited it.
struct Inherited {
    env: BTreeMap<String, String>,
    inner: LocalTreeTransport,
}

#[ersa_adapter::async_trait]
impl Transport for Inherited {
    async fn spawn(&self, mut spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        spec.env.extend(self.env.clone());
        self.inner.spawn(spec).await
    }
}

/// Adds a repository under `dir` as a project and asks for a workspace in
/// it, which runs `git worktree add` and so the post-checkout hook.
async fn create_workspace(client: &mut Client, dir: &Path) -> Value {
    let repo = dir.join("repo");
    git_repo(&repo);
    let project = client
        .result(
            "project.add",
            json!({ "host_id": "local", "path": repo.display().to_string(), "trusted": false }),
        )
        .await;
    client
        .call("workspace.create", json!({ "project_id": project["id"] }))
        .await
}

/// The check is real: git that is not sealed reads the hostile home, both
/// a test's own commit and a Core's worktree.
#[tokio::test(flavor = "multi_thread")]
async fn a_hostile_home_reaches_git_that_is_not_sealed() {
    let dir = tempdir();
    let hostile = sealed::hostile_home(dir.path());

    let repo = dir.path().join("repo");
    git_repo(&repo);
    std::fs::write(repo.join("change.txt"), "a change\n").unwrap();
    let git = || {
        let mut git = Command::new("git");
        git.arg("-C").arg(&repo).envs(&hostile);
        git.args([
            "-c",
            "user.name=Ersa Tests",
            "-c",
            "user.email=tests@ersa.invalid",
        ]);
        git
    };
    assert!(
        git()
            .args(["add", "change.txt"])
            .status()
            .unwrap()
            .success()
    );
    let commit = git().args(["commit", "--quiet", "-m", "a change"]).status();
    assert!(
        !commit.unwrap().success(),
        "the hostile home stops a commit"
    );

    // A Core bound here, not through the test support, so not sealed.
    let endpoint = unique_endpoint();
    let mut config = test_config(&dir.path().join("data"), &endpoint);
    config.command_transport = Some(CommandTransport(Arc::new(Inherited {
        env: hostile,
        inner: LocalTreeTransport::for_commands(),
    })));
    let server = Server::bind_with(config, Vec::new()).await.expect("bind");
    let task = tokio::spawn(server.run());
    let mut client = Client::connect(&endpoint).await;
    client.hello().await;
    let response = create_workspace(&mut client, &dir.path().join("core")).await;
    assert_eq!(kind(&response["error"]), "vcs_error", "{response:#}");
    client.result("core.shutdown", json!({})).await;
    task.await.unwrap().unwrap();
}

/// An ordinary test: the test support commits a repository and its Core
/// makes a worktree of it.
#[tokio::test(flavor = "multi_thread")]
async fn a_test_commits_and_its_core_makes_a_worktree() {
    let core = start().await;
    let mut client = core.client().await;
    let response = create_workspace(&mut client, core.dir.path()).await;
    assert!(response.get("result").is_some(), "{response:#}");
}

/// The same test, run where git would read the hostile home: the test
/// support's git and its Core's git are sealed, so it passes.
#[test]
fn a_test_and_its_core_ignore_a_global_hook_and_signing() {
    sealed::passes_in_a_hostile_home("a_test_commits_and_its_core_makes_a_worktree");
}
