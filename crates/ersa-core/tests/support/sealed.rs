//! Git sealed from the configuration of whoever runs the tests.
//!
//! Without this, a test's git (and the git of a Core a test starts) reads the
//! user's global and system configuration: their hooks, file-system monitor
//! and commit signing reach test repositories, and a test passes on one
//! machine and fails on another. Production Core keeps reading the user's
//! configuration; only tests are sealed.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use ersa_adapter::{Process, ProcessSpec, Transport};

/// The variables that seal git. The global configuration is empty (Git for
/// Windows reads `/dev/null` as its null device too), and neither the system
/// configuration (Git for Windows' `etc/gitconfig` and `ProgramData` file
/// included) nor the system attributes are read. The identity a commit needs
/// comes from git's own identity variables, not from any configuration.
pub const SET: [(&str, &str); 7] = [
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_ATTR_NOSYSTEM", "1"),
    ("GIT_AUTHOR_NAME", "Ersa Tests"),
    ("GIT_AUTHOR_EMAIL", "tests@ersa.invalid"),
    ("GIT_COMMITTER_NAME", "Ersa Tests"),
    ("GIT_COMMITTER_EMAIL", "tests@ersa.invalid"),
];

/// Variables that would carry configuration in from the environment the
/// tests run in (a git hook running `cargo test` exports them), removed.
pub const REMOVED: [&str; 2] = ["GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT"];

/// A `git -C <dir>` command, sealed.
pub fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    for name in REMOVED {
        command.env_remove(name);
    }
    command.arg("-C").arg(dir).envs(SET);
    command
}

/// A command transport that seals every process it starts, then hands it to
/// `inner`. The test support wraps every test Core's command transport in it
/// (`support::seal`), so Core's own git calls are sealed too.
pub struct SealedTransport {
    pub inner: Arc<dyn Transport>,
}

#[ersa_adapter::async_trait]
impl Transport for SealedTransport {
    async fn spawn(&self, mut spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        spec.env_remove.extend(REMOVED.map(String::from));
        for (name, value) in SET {
            spec.env.insert(name.to_string(), value.to_string());
        }
        self.inner.spawn(spec).await
    }
}

/// The environment that points git at a hostile home under `dir`: its
/// global configuration runs a failing `pre-commit` and `post-checkout`
/// hook and signs every commit with a program that does not exist, and a
/// system configuration file says the same. Git that reads either fails to
/// commit and to make a worktree.
pub fn hostile_home(dir: &Path) -> BTreeMap<String, String> {
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

/// Runs the test named `test` of this test binary again, alone, in a child
/// process that inherits a hostile home ([`hostile_home`]), and fails unless
/// it passes there. Everything that test does runs as it would on the
/// machine of a user with that configuration: the test support's own git,
/// and every git call of each Core it starts.
pub fn passes_in_a_hostile_home(test: &str) {
    let dir = super::tempdir();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .envs(hostile_home(dir.path()))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // A filter that names no test passes too, so the count is checked.
    assert!(
        output.status.success() && stdout.contains("test result: ok. 1 passed"),
        "{test} fails in a hostile home:\n{stdout}\n{stderr}"
    );
}
