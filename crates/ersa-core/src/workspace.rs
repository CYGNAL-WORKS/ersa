//! Workspace and VCS: creates and destroys isolated workspaces for a project
//! behind a VCS interface. Git worktrees first, Perforce second. Executes through
//! the transport so remote hosts work the same way as the local one
//! (ARCH-CORE-R05).
//!
//! Only git exists so far, as plain functions. A `Vcs` trait arrives with the
//! second implementation (Perforce, milestone M4), when its shape is known.
//!
//! Decisions here read git's machine-readable output (`--porcelain`), never
//! its messages, which change between versions and languages. Messages are
//! only passed on to the user.

use std::time::Duration;

use ersa_adapter::{ProcessSpec, Transport};

use crate::platform;
use crate::transport::{self, Output, RunError};

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
    /// git could not be started.
    #[error("could not run git: {0}")]
    Spawn(std::io::Error),
    /// git started but did not finish in time, or its output could not be
    /// read; what it was doing may be half done.
    #[error("git did not complete: {0}")]
    Incomplete(std::io::Error),
    /// git ran and failed; `stderr` says why.
    #[error("{message}")]
    Failed { message: String, stderr: String },
    /// The worktree is locked (`git worktree lock`), which protects it from
    /// removal until someone unlocks it.
    #[error("the worktree is locked{}", .reason.as_deref().map(|r| format!(": {r}")).unwrap_or_default())]
    Locked { reason: Option<String> },
    /// The worktree has local changes and removal was not forced.
    #[error("the worktree has uncommitted changes or untracked files")]
    Dirty,
}

impl VcsError {
    /// Whether the worktree was left exactly as it was: git refused before
    /// touching it.
    pub fn is_refusal(&self) -> bool {
        matches!(self, VcsError::Locked { .. } | VcsError::Dirty)
    }
}

impl From<RunError> for VcsError {
    fn from(e: RunError) -> Self {
        match e {
            RunError::Spawn(io) => VcsError::Spawn(io),
            RunError::Incomplete(io) => VcsError::Incomplete(io),
        }
    }
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

/// Removes the git worktree at `path`, as far as possible, after an add that
/// did not complete: forced removal, then `prune` for git's own records.
/// Errors are returned so the caller can tell whether anything may be left.
pub async fn git_worktree_discard(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
) -> Result<(), VcsError> {
    let removed = git(
        transport,
        repo,
        &["worktree", "remove", "--force", "--", path],
    )
    .await?;
    let pruned = git(transport, repo, &["worktree", "prune"]).await?;
    // The removal fails harmlessly when git never registered the worktree;
    // what matters is that git no longer lists it afterwards.
    let listed = git_worktree_list(transport, repo).await?;
    if find(&listed, path).is_some() {
        check(removed, "git worktree remove failed")?;
        check(pruned, "git worktree prune failed")?;
    }
    Ok(())
}

/// Removes the git worktree at `path`.
///
/// - Not a worktree git knows (already removed, or never finished): its
///   record is pruned and this succeeds.
/// - Locked: refused with [`VcsError::Locked`], even with `force`.
/// - Local changes and no `force`: refused with [`VcsError::Dirty`].
/// - Otherwise removed, retried while files are busy.
///
/// The worktree's branch is kept, so committed work is never lost.
pub async fn git_worktree_remove(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    force: bool,
) -> Result<(), VcsError> {
    let listed = git_worktree_list(transport, repo).await?;
    let Some(entry) = find(&listed, path) else {
        let pruned = git(transport, repo, &["worktree", "prune"]).await?;
        return check(pruned, "git worktree prune failed");
    };
    if entry.locked {
        return Err(VcsError::Locked {
            reason: entry.lock_reason.clone(),
        });
    }
    if entry.prunable {
        // The folder is gone; only git's record remains.
        let pruned = git(transport, repo, &["worktree", "prune"]).await?;
        return check(pruned, "git worktree prune failed");
    }
    if !force && is_dirty(transport, path).await? {
        return Err(VcsError::Dirty);
    }

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
        if attempt == REMOVE_ATTEMPTS {
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

/// Whether the worktree at `path` has anything `git worktree remove` would
/// refuse over: modified or untracked files. Ignored files do not count.
async fn is_dirty(transport: &dyn Transport, path: &str) -> Result<bool, VcsError> {
    let output = git(transport, path, &["status", "--porcelain"]).await?;
    if !output.exit.success() {
        return Err(VcsError::Failed {
            message: "git status failed".into(),
            stderr: output.stderr_text(),
        });
    }
    Ok(output.stdout.iter().any(|line| !line.is_empty()))
}

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub locked: bool,
    pub lock_reason: Option<String>,
    /// The folder is gone and only git's record of it remains.
    pub prunable: bool,
}

/// The repository's worktrees, the main one included.
pub async fn git_worktree_list(
    transport: &dyn Transport,
    repo: &str,
) -> Result<Vec<WorktreeEntry>, VcsError> {
    let output = git(transport, repo, &["worktree", "list", "--porcelain"]).await?;
    if !output.exit.success() {
        return Err(VcsError::Failed {
            message: "git worktree list failed".into(),
            stderr: output.stderr_text(),
        });
    }
    Ok(parse_worktree_list(&output.stdout))
}

/// Parses `git worktree list --porcelain`: one block per worktree, blocks
/// separated by blank lines, each starting `worktree <path>` and holding
/// attribute lines such as `locked [<reason>]` and `prunable [<reason>]`.
fn parse_worktree_list(lines: &[String]) -> Vec<WorktreeEntry> {
    let mut entries = Vec::new();
    let mut current: Option<WorktreeEntry> = None;
    for line in lines {
        let (key, value) = match line.split_once(' ') {
            Some((key, value)) => (key, Some(value)),
            None => (line.as_str(), None),
        };
        match key {
            "worktree" => {
                entries.extend(current.take());
                current = Some(WorktreeEntry {
                    path: value.unwrap_or_default().to_string(),
                    ..WorktreeEntry::default()
                });
            }
            "locked" => {
                if let Some(entry) = current.as_mut() {
                    entry.locked = true;
                    entry.lock_reason = value.map(str::to_string);
                }
            }
            "prunable" => {
                if let Some(entry) = current.as_mut() {
                    entry.prunable = true;
                }
            }
            _ => {}
        }
    }
    entries.extend(current);
    entries
}

/// The entry for `path`, comparing paths the way the local file system does.
/// git prints `D:/x` where Core stores `D:\x`.
fn find<'a>(entries: &'a [WorktreeEntry], path: &str) -> Option<&'a WorktreeEntry> {
    let key = platform::path_key(path);
    entries
        .iter()
        .find(|entry| platform::path_key(&entry.path) == key)
}

/// Runs `git -C <dir> <args>` with a fixed environment.
async fn git(transport: &dyn Transport, dir: &str, args: &[&str]) -> Result<Output, VcsError> {
    let mut all = vec!["-C".to_string(), dir.to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    let env = [
        // English, unlocalised messages: what reaches the user and the logs
        // reads the same everywhere.
        ("LC_ALL", "C"),
        ("LANGUAGE", "C"),
        // Never stop to ask for credentials; there is nobody to answer.
        ("GIT_TERMINAL_PROMPT", "0"),
    ];
    let spec = ProcessSpec {
        program: "git".into(),
        args: all,
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_list_is_parsed() {
        let text = "worktree D:/repo\nHEAD abc\nbranch refs/heads/main\n\n\
                    worktree D:/wt/a\nHEAD def\nbranch refs/heads/a\nlocked on a usb drive\n\n\
                    worktree D:/wt/b\nHEAD 123\ndetached\nlocked\nprunable gitdir file points to non-existent location\n";
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let entries = parse_worktree_list(&lines);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, "D:/repo");
        assert!(!entries[0].locked);
        assert!(entries[1].locked);
        assert_eq!(entries[1].lock_reason.as_deref(), Some("on a usb drive"));
        assert!(entries[2].locked && entries[2].lock_reason.is_none());
        assert!(entries[2].prunable);
    }

    #[test]
    fn locked_errors_name_the_reason() {
        let e = VcsError::Locked {
            reason: Some("in use".into()),
        };
        assert_eq!(e.to_string(), "the worktree is locked: in use");
        assert!(e.is_refusal());
        assert!(VcsError::Dirty.is_refusal());
    }
}
