//! Git sealed from the configuration of whoever runs the tests.
//!
//! Without this, a test's git (and the git of a Core a test starts) reads the
//! user's global and system configuration: their hooks, file-system monitor
//! and commit signing reach test repositories, and a test passes on one
//! machine and fails on another. Production Core keeps reading the user's
//! configuration; only tests are sealed.

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
