//! Workspace and VCS: creates and destroys isolated workspaces for a project
//! behind a VCS interface. Git worktrees first, Perforce second. Executes through
//! the transport so remote hosts work the same way as the local one
//! (ARCH-CORE-R05).
//!
//! Only git exists so far, as plain functions. A `Vcs` trait arrives with the
//! second implementation (Perforce, milestone M4), when its shape is known.

use std::time::Duration;

use ersa_adapter::{ProcessSpec, Transport};

use crate::transport::{self, Output};

/// How long one git command may take. Creating a worktree checks out the
/// whole tree, which takes a while for a large repository.
const GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// How many times removal is tried, and the wait before the first retry
/// (doubled each time). On Windows, a process that has just exited (an editor,
/// an indexer, a session's backend) can hold files in the worktree for a
/// moment, which makes the first removal fail.
const REMOVE_ATTEMPTS: u32 = 4;
const REMOVE_FIRST_BACKOFF: Duration = Duration::from_millis(250);

/// Why a VCS operation failed.
#[derive(Debug, thiserror::Error)]
pub enum VcsError {
    /// The VCS tool could not be run at all.
    #[error("could not run git: {0}")]
    Spawn(#[from] std::io::Error),
    /// It ran and refused; `stderr` says why.
    #[error("{message}")]
    Failed { message: String, stderr: String },
}

/// Creates a git worktree at `path` on a new branch `branch`, starting from
/// `base` (the repository's HEAD when `None`).
pub async fn git_worktree_add(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    branch: &str,
    base: Option<&str>,
) -> Result<(), VcsError> {
    // `--` ends option parsing, so a path or base can never be read as a flag.
    let mut args = vec!["worktree", "add", "-b", branch, "--", path];
    args.extend(base);
    let output = git(transport, repo, &args).await?;
    check(output, "git worktree add failed")
}

/// Removes the git worktree at `path`: retried while the files are busy, and
/// pruned instead when the directory has already gone. With `force`, local
/// changes in the worktree are discarded; without it, git refuses to remove a
/// worktree that has any.
///
/// The worktree's branch is kept, so committed work is never lost.
pub async fn git_worktree_remove(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    force: bool,
) -> Result<(), VcsError> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", path]);

    let mut backoff = REMOVE_FIRST_BACKOFF;
    let mut attempt = 1;
    loop {
        let output = git(transport, repo, &args).await?;
        if output.exit.success() {
            return Ok(());
        }
        let stderr = output.stderr_text();
        if stderr.contains("is not a working tree") {
            // Already gone from disk (deleted by hand): drop git's record of it.
            let pruned = git(transport, repo, &["worktree", "prune"]).await?;
            return check(pruned, "git worktree prune failed");
        }
        // A refusal over local changes will not change by waiting.
        let refused = stderr.contains("--force");
        if refused || attempt == REMOVE_ATTEMPTS {
            return Err(VcsError::Failed {
                message: "git worktree remove failed".into(),
                stderr,
            });
        }
        tracing::debug!(attempt, %stderr, "worktree removal failed; retrying");
        tokio::time::sleep(backoff).await;
        backoff *= 2;
        attempt += 1;
    }
}

/// Runs `git -C <repo> <args>`.
async fn git(transport: &dyn Transport, repo: &str, args: &[&str]) -> Result<Output, VcsError> {
    let mut all = vec!["-C".to_string(), repo.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    let spec = ProcessSpec {
        program: "git".into(),
        args: all,
        ..ProcessSpec::default()
    };
    Ok(transport::run(transport, spec, GIT_TIMEOUT).await?)
}

fn check(output: Output, message: &str) -> Result<(), VcsError> {
    if output.exit.success() {
        Ok(())
    } else {
        Err(VcsError::Failed {
            message: message.into(),
            stderr: output.stderr_text(),
        })
    }
}
