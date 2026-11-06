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

/// How long a quick question to git may take: listing branches, resolving a
/// base. The Interface waits on these while the user picks, so a repository
/// that hangs (a network drive, a stuck lock) is reported rather than waited
/// on.
const GIT_QUERY_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// A folder is at the workspace's path, but git does not list it as one
    /// of the repository's worktrees, so removing it is not git's to do.
    #[error("{path} exists but git does not list it as a worktree; remove the folder by hand")]
    NotListed { path: String },
}

impl VcsError {
    /// Whether the worktree was left exactly as it was: git refused before
    /// touching it.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            VcsError::Locked { .. } | VcsError::Dirty | VcsError::NotListed { .. }
        )
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
/// the commit `base`.
pub async fn git_worktree_add(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    branch: &str,
    base: &str,
) -> Result<(), VcsError> {
    // `--` ends option parsing, so a path or base can never be read as a flag.
    let args = ["worktree", "add", "-b", branch, "--", path, base];
    let output = git(transport, repo, &args).await?;
    check(output, "git worktree add failed")
}

/// A repository's local branches, and which one is its default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalBranches {
    /// Short names (`main`, `feature/rail`), sorted.
    pub names: Vec<String>,
    /// The default branch, one of `names`: the one `origin/HEAD` points
    /// at, else `main`, else `master`, else the checked-out branch. None
    /// on a detached checkout with none of those.
    pub default: Option<String>,
}

/// The local branches of the repository at `repo`, and its default branch.
/// One git call, given [`GIT_QUERY_TIMEOUT`].
pub async fn git_local_branches(
    transport: &dyn Transport,
    repo: &str,
) -> Result<LocalBranches, VcsError> {
    // Each line is `<ref>:<* if checked out>:<target if a symbolic ref>`.
    // A ref name can never contain `:`, so the split is unambiguous. The
    // second pattern names `origin/HEAD` exactly, to read where it points.
    let format = "--format=%(refname):%(HEAD):%(symref)";
    let args = [
        "for-each-ref",
        format,
        "refs/heads",
        "refs/remotes/origin/HEAD",
    ];
    let output = query(transport, repo, &args, deadline()).await?;
    if !output.exit.success() {
        return Err(VcsError::Failed {
            message: "git could not list the branches".into(),
            stderr: output.stderr_text(),
        });
    }
    Ok(parse_branches(&output.stdout))
}

/// Parses `git_local_branches`' `for-each-ref` output and picks the default.
fn parse_branches(lines: &[String]) -> LocalBranches {
    let mut names = Vec::new();
    let mut checked_out = None;
    let mut origin_head = None;
    for line in lines {
        let mut fields = line.splitn(3, ':');
        let (Some(refname), Some(head), Some(symref)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if refname == "refs/remotes/origin/HEAD" {
            origin_head = symref
                .strip_prefix("refs/remotes/origin/")
                .map(str::to_string);
        } else if let Some(name) = refname.strip_prefix("refs/heads/") {
            if head == "*" {
                checked_out = Some(name.to_string());
            }
            names.push(name.to_string());
        }
    }
    names.sort();
    // `origin/HEAD` counts only when the branch it names exists locally:
    // a base is a local branch.
    let local = |name: &str| names.iter().any(|n| n == name);
    let default = origin_head
        .filter(|name| local(name))
        .or_else(|| {
            ["main", "master"]
                .into_iter()
                .find(|n| local(n))
                .map(str::to_string)
        })
        .or(checked_out);
    LocalBranches { names, default }
}

/// The commit `revision` names in the repository at `repo`, or `None`
/// when git cannot resolve it to a commit. All of it, two git calls at
/// most, is given [`GIT_QUERY_TIMEOUT`].
///
/// The caller refuses a revision that starts with `-`, so git reads it as a
/// revision, never as an option.
pub async fn git_resolve_commit(
    transport: &dyn Transport,
    repo: &str,
    revision: &str,
) -> Result<Option<String>, VcsError> {
    // `^{commit}` peels a tag to the commit it tags and refuses anything
    // that is not a commit (a tree, a blob).
    let peeled = format!("{revision}^{{commit}}");
    let deadline = deadline();
    let args = ["rev-parse", "--verify", "--quiet", &peeled];
    let output = query(transport, repo, &args, deadline).await?;
    let failed = || VcsError::Failed {
        message: "git could not resolve the base".into(),
        stderr: output.stderr_text(),
    };
    match output.exit.code {
        Some(0) => return Ok(output.stdout.first().map(|line| line.trim().to_string())),
        // `--verify --quiet` usually exits 1 for what it cannot resolve, but
        // some revisions it cannot resolve exit 128 (a reflog entry past the
        // reflog's end), the same code as a broken repository; checked below.
        Some(1 | 128) => {}
        // Killed, or failing in a way that says nothing about the revision.
        _ => return Err(failed()),
    }
    // The exit code cannot tell an unresolvable revision from a broken
    // repository: ask git whether the repository itself is usable. If it
    // is, the revision was the problem.
    let repository = query(transport, repo, &["rev-parse", "--git-dir"], deadline).await?;
    if repository.exit.success() {
        return Ok(None);
    }
    Err(failed())
}

/// When a git query started now must have answered.
fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + GIT_QUERY_TIMEOUT
}

/// Runs a quick git query that must answer by `deadline`, which several
/// queries for one answer share, so together they stay within
/// [`GIT_QUERY_TIMEOUT`]. Running out of time is git's failure, reported as
/// such: a query changes nothing, so there is nothing half done to clean up.
async fn query(
    transport: &dyn Transport,
    dir: &str,
    args: &[&str],
    deadline: tokio::time::Instant,
) -> Result<Output, VcsError> {
    let left = deadline.saturating_duration_since(tokio::time::Instant::now());
    match git_within(transport, dir, args, left).await {
        Err(VcsError::Incomplete(e)) if e.kind() == std::io::ErrorKind::TimedOut => {
            Err(VcsError::Failed {
                message: format!(
                    "git did not answer within {} seconds",
                    GIT_QUERY_TIMEOUT.as_secs()
                ),
                stderr: e.to_string(),
            })
        }
        other => other,
    }
}

/// The lock reason `git worktree add` writes while it works and removes
/// when it finishes. Still present afterwards means the add was killed.
const INITIALIZING: &str = "initializing";

/// Removes the git worktree at `path`, as far as possible, after an add that
/// did not complete: forced removal, then `prune` for git's own records.
/// Errors are returned so the caller can tell whether anything may be left.
pub async fn git_worktree_discard(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
) -> Result<(), VcsError> {
    // A killed add leaves its `initializing` lock behind. One `--force`
    // ignores local changes; the second also overrides a lock.
    let removed = git(
        transport,
        repo,
        &["worktree", "remove", "--force", "--force", "--", path],
    )
    .await?;
    let pruned = git(transport, repo, &["worktree", "prune"]).await?;
    // The removal fails harmlessly when git never registered the worktree;
    // what matters is that git no longer lists it afterwards.
    let listed = git_worktree_list(transport, repo).await?;
    if find(&listed, path).await.is_some() {
        check(removed, "git worktree remove failed")?;
        check(pruned, "git worktree prune failed")?;
    }
    Ok(())
}

/// Removes the git worktree at `path`.
///
/// - Not a worktree git lists: if the folder is gone too, git's record is
///   pruned and this succeeds; if the folder is still there, it is refused
///   with [`VcsError::NotListed`] rather than reported removed.
/// - Locked: refused with [`VcsError::Locked`], even with `force`. The one
///   exception is a lock left by an add that was killed (reason
///   `initializing`) when `after_failed_add` says the workspace never
///   finished: that worktree holds nothing of the user's and is discarded.
/// - Local changes and no `force`: refused with [`VcsError::Dirty`].
/// - Otherwise removed, retried while files are busy.
///
/// The worktree's branch is kept, so committed work is never lost.
pub async fn git_worktree_remove(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    force: bool,
    after_failed_add: bool,
) -> Result<(), VcsError> {
    let listed = git_worktree_list(transport, repo).await?;
    let Some(entry) = find(&listed, path).await else {
        if exists(path).await {
            return Err(VcsError::NotListed {
                path: path.to_string(),
            });
        }
        let pruned = git(transport, repo, &["worktree", "prune"]).await?;
        return check(pruned, "git worktree prune failed");
    };
    if entry.locked {
        let interrupted = entry.lock_reason.as_deref() == Some(INITIALIZING);
        if interrupted && after_failed_add {
            return git_worktree_discard(transport, repo, path).await;
        }
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

/// The entry for `path`.
///
/// git prints each worktree's real path: links and junctions resolved, and
/// `/` separators on Windows. So both sides are canonicalised before they are
/// compared, falling back to the text when a folder no longer exists (a
/// prunable entry, or a path already deleted). This reads the local file
/// system: worktrees are on the local host until remote hosts (M4), whose
/// paths will need the comparison run on that host.
async fn find(entries: &[WorktreeEntry], path: &str) -> Option<WorktreeEntry> {
    let entries = entries.to_vec();
    let path = path.to_string();
    tokio::task::spawn_blocking(move || {
        let key = location_key(&path);
        entries.into_iter().find(|e| location_key(&e.path) == key)
    })
    .await
    .ok()
    .flatten()
}

/// The key under which two spellings of one folder compare equal: the key a
/// project's folder is stored under, or the path's own text for a folder that
/// no longer exists. Blocking.
pub(crate) fn location_key(path: &str) -> String {
    match platform::canonicalize(std::path::Path::new(path)) {
        Ok(canonical) => platform::folder_key(&canonical),
        Err(_) => platform::path_key(path),
    }
}

/// Whether anything exists at `path` on the local file system.
async fn exists(path: &str) -> bool {
    let path = path.to_string();
    tokio::task::spawn_blocking(move || std::path::Path::new(&path).exists())
        .await
        .unwrap_or(true)
}

/// Runs `git -C <dir> <args>` with a fixed environment, under [`GIT_TIMEOUT`].
async fn git(transport: &dyn Transport, dir: &str, args: &[&str]) -> Result<Output, VcsError> {
    git_within(transport, dir, args, GIT_TIMEOUT).await
}

/// Runs `git -C <dir> <args>` with a fixed environment, killing it after
/// `timeout`.
async fn git_within(
    transport: &dyn Transport,
    dir: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, VcsError> {
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
    Ok(transport::run(transport, spec, timeout).await?)
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

    // verifies: PROTO-CORE-API-R18
    #[test]
    fn main_comes_before_master() {
        // The socket tests cover each rule alone; this is the one case they
        // don't: both exist, with another branch checked out.
        let text = "refs/heads/main: :\nrefs/heads/master: :\nrefs/heads/x:*:";
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let parsed = parse_branches(&lines);
        assert_eq!(parsed.names, ["main", "master", "x"]);
        assert_eq!(parsed.default.as_deref(), Some("main"));
    }

    /// A transport whose git never answers: its output never ends.
    fn hanging_git() -> ersa_adapter::fake::FakeTransport {
        ersa_adapter::fake::FakeTransport::new(Vec::<String>::new()).stdout_outlives_process()
    }

    /// Asserts `result` is git's failure from running out of time, reached
    /// exactly at the 10-second limit. The clock is paused, so tokio jumps it
    /// to each timer and no test waits ten real seconds.
    fn timed_out_at_ten_seconds<T: std::fmt::Debug>(
        result: Result<T, VcsError>,
        started: tokio::time::Instant,
    ) {
        assert_eq!(started.elapsed(), Duration::from_secs(10));
        match result {
            // `Failed` is what the Core API answers as `vcs_error`.
            Err(VcsError::Failed { message, .. }) => {
                assert!(message.contains("10 seconds"), "{message}")
            }
            other => panic!("expected a timed-out git failure, got {other:?}"),
        }
    }

    // verifies: PROTO-CORE-API-R18
    #[tokio::test(start_paused = true)]
    async fn listing_branches_gives_git_ten_seconds() {
        let started = tokio::time::Instant::now();
        let result = git_local_branches(&hanging_git(), "repo").await;
        timed_out_at_ten_seconds(result, started);
    }

    // verifies: PROTO-CORE-API-R19
    #[tokio::test(start_paused = true)]
    async fn resolving_a_base_gives_git_ten_seconds() {
        let started = tokio::time::Instant::now();
        let result = git_resolve_commit(&hanging_git(), "repo", "main").await;
        timed_out_at_ten_seconds(result, started);
    }

    // verifies: PROTO-CORE-API-R19
    #[tokio::test(start_paused = true)]
    async fn resolving_a_base_gives_both_git_calls_ten_seconds_together() {
        /// The first git exits 128 after six seconds; the second never answers.
        struct SlowThenHanging(std::sync::atomic::AtomicUsize);
        #[ersa_adapter::async_trait]
        impl Transport for SlowThenHanging {
            async fn spawn(
                &self,
                spec: ProcessSpec,
            ) -> std::io::Result<Box<dyn ersa_adapter::Process>> {
                if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    tokio::time::sleep(Duration::from_secs(6)).await;
                    let failing = ersa_adapter::fake::FakeTransport::new(Vec::<String>::new())
                        .with_exit_code(128);
                    return failing.spawn(spec).await;
                }
                hanging_git().spawn(spec).await
            }
        }
        let transport = SlowThenHanging(Default::default());
        let started = tokio::time::Instant::now();
        let result = git_resolve_commit(&transport, "repo", "main@{50}").await;
        timed_out_at_ten_seconds(result, started);
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
