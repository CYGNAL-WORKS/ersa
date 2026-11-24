//! Tests' git is sealed from the configuration of whoever runs them
//! (ARCH-CORE, Verification): a global hook or commit signing in the user's
//! home reaches neither a test's own git nor the git of a Core a test starts.
//!
//! Each test sets up a hostile home whose global configuration fails every
//! commit and checkout, shows it bites git that is not sealed, then shows
//! sealed git ignores it.

mod support;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use ersa_adapter::{Process, ProcessSpec, Transport};
use ersa_core::transport::LocalTreeTransport;
use ersa_core::{CommandTransport, CoreConfig, Server};
use serde_json::{Value, json};
use support::{Client, git_repo, kind, sealed, start_with, tempdir, test_config, unique_endpoint};

/// The environment that points git at a hostile home: its global
/// configuration runs a failing `pre-commit` and `post-checkout` hook and
/// signs every commit with a program that does not exist, and a system
/// configuration file says the same.
fn hostile_home(dir: &Path) -> BTreeMap<String, String> {
    let home = dir.join("home");
    let hooks = home.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for hook in ["pre-commit", "post-checkout"] {
        let path = hooks.join(hook);
        std::fs::write(&path, "#!/bin/sh\necho the global hook ran >&2\nexit 1\n").unwrap();
        // Git runs a hook only when it is executable.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let config = format!(
        "[core]\n\thooksPath = {}\n[commit]\n\tgpgsign = true\n[gpg]\n\tprogram = ersa-test-no-such-gpg\n",
        hooks.display().to_string().replace('\\', "/")
    );
    std::fs::write(home.join(".gitconfig"), &config).unwrap();
    let system = dir.join("system-gitconfig");
    std::fs::write(&system, &config).unwrap();

    let home = home.display().to_string();
    BTreeMap::from([
        // Git reads `HOME` on every platform, Git for Windows included.
        ("HOME".to_string(), home.clone()),
        ("USERPROFILE".to_string(), home.clone()),
        ("XDG_CONFIG_HOME".to_string(), format!("{home}/.config")),
        (
            "GIT_CONFIG_SYSTEM".to_string(),
            system.display().to_string(),
        ),
    ])
}

/// Stages a file in `repo` and commits it with `git`; whether it worked.
fn commits(repo: &Path, mut git: impl FnMut() -> Command) -> bool {
    std::fs::write(repo.join("change.txt"), "a change\n").unwrap();
    assert!(
        git()
            .args(["add", "change.txt"])
            .status()
            .unwrap()
            .success()
    );
    git()
        .args(["commit", "--quiet", "-m", "a change"])
        .status()
        .unwrap()
        .success()
}

#[test]
fn a_tests_git_ignores_a_global_hook_and_signing() {
    let dir = tempdir();
    let repo = dir.path().join("repo");
    git_repo(&repo);
    let hostile = hostile_home(dir.path());

    // Not sealed, the hostile home stops the commit: the check is real.
    let bare = || {
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
        !commits(&repo, bare),
        "the hostile home reaches unsealed git"
    );

    // Sealed, as the test support's git runs, the commit goes through.
    let sealed = || {
        let mut git = sealed::git_command(&repo);
        git.envs(&hostile);
        git
    };
    assert!(commits(&repo, sealed), "sealed git reads the hostile home");
}

/// Core's command transport with the hostile home added to every process
/// it starts, as if Core had inherited it.
struct HostileHome {
    env: BTreeMap<String, String>,
    inner: LocalTreeTransport,
}

#[ersa_adapter::async_trait]
impl Transport for HostileHome {
    async fn spawn(&self, mut spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        spec.env.extend(self.env.clone());
        self.inner.spawn(spec).await
    }
}

fn hostile_transport(dir: &Path) -> CommandTransport {
    CommandTransport(Arc::new(HostileHome {
        env: hostile_home(dir),
        inner: LocalTreeTransport::for_commands(),
    }))
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

#[tokio::test(flavor = "multi_thread")]
async fn a_core_a_test_starts_ignores_a_global_hook() {
    // Not sealed (bound here, not through the test support), the hostile
    // home stops Core's worktree: the check is real.
    let dir = tempdir();
    let endpoint = unique_endpoint();
    let mut config: CoreConfig = test_config(&dir.path().join("data"), &endpoint);
    config.command_transport = Some(hostile_transport(dir.path()));
    let server = Server::bind_with(config, Vec::new()).await.expect("bind");
    let task = tokio::spawn(server.run());
    let mut client = Client::connect(&endpoint).await;
    client.hello().await;
    let response = create_workspace(&mut client, dir.path()).await;
    assert_eq!(kind(&response["error"]), "vcs_error", "{response:#}");
    client.result("core.shutdown", json!({})).await;
    task.await.unwrap().unwrap();

    // Started through the test support, Core's git is sealed and the
    // workspace is made.
    let dir = tempdir();
    let hostile = hostile_transport(dir.path());
    let core = start_with(dir, |config| config.command_transport = Some(hostile)).await;
    let mut client = core.client().await;
    let response = create_workspace(&mut client, core.dir.path()).await;
    assert!(response.get("result").is_some(), "{response:#}");
}
