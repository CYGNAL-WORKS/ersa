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

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{CallOff, CallOffOnDrop, ProcessSpec, Removal, Transport, remove_tree};

use crate::platform;
use crate::state::Gate;
use crate::transport::{self, Output, RunError};

/// How long one git command may take. Creating a worktree checks out the
/// whole tree, which takes a while for a large repository.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(300);

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

/// How long `session.delete` goes on trying a workspace's folder: no try
/// begins later than this after the first (PROTO-CORE-API-R113). A process
/// that has just exited, or one reading the session's files, can hold them
/// for several seconds after the session stops.
pub const FOLDER_TRY_WINDOW: Duration = Duration::from_secs(30);

/// The longest wait between two of those tries; the waits start at
/// [`REMOVE_FIRST_BACKOFF`] and double up to this.
const FOLDER_TRY_LONGEST_WAIT: Duration = Duration::from_secs(2);

/// How long a request waits for its turn to change a repository's
/// worktrees while another change to them is under way ([`Worktrees`]),
/// counted from when it began to wait: as long as one git command may take
/// (PROTO-CORE-API-R119).
pub const WORKTREE_TURN_WAIT: Duration = GIT_TIMEOUT;

/// A repository's turns to have its worktrees changed. git cannot add,
/// remove or prune two worktrees of one repository at once: each lists the
/// repository's worktrees and dies on the record of one another git is
/// still writing ("failed to read .git/worktrees/<name>/commondir"). So
/// every change to them waits for its turn.
#[ersa_adapter::async_trait]
pub trait Worktrees: Send + Sync {
    /// Waits for the turn, at most [`WORKTREE_TURN_WAIT`] (or the test's
    /// limit), and answers
    /// [`VcsError::WorktreesBusy`] past it. Keep what it returns while git
    /// changes the repository's worktrees, and let go of it after.
    async fn turn(&self) -> Result<Box<dyn Send>, VcsError>;
}

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
    /// Too many earlier requests' file system reads have not answered yet.
    #[error("too many workspace folders asked about are not answering; try again once they do")]
    Busy,
    /// Another change to the repository's worktrees did not finish within
    /// [`WORKTREE_TURN_WAIT`]; this one began no git work.
    #[error("another change to this project's workspaces did not finish in time; try again")]
    WorktreesBusy,
}

impl VcsError {
    /// Whether the worktree was left exactly as it was: git refused before
    /// touching it, or a removal that never had its turn ran no git.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            VcsError::Locked { .. }
                | VcsError::Dirty
                | VcsError::NotListed { .. }
                | VcsError::WorktreesBusy
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

/// What a workspace made without a base starts from in the repository at
/// `repo`: the revision naming its default branch ([`default_branch`]), or
/// `HEAD` on a detached checkout with none, and the commit that revision
/// names, `None` when it names none (a repository with no commit yet). All
/// of it, three git calls at most, is given [`GIT_QUERY_TIMEOUT`] together.
pub async fn git_default_commit(
    transport: &dyn Transport,
    repo: &str,
) -> Result<(String, Option<String>), VcsError> {
    let deadline = deadline();
    let revision = match default_branch(transport, repo, deadline).await? {
        Some(name) => format!("refs/heads/{name}"),
        None => "HEAD".to_string(),
    };
    let commit = resolve_commit(transport, repo, &revision, deadline).await?;
    Ok((revision, commit))
}

/// The default branch of the repository at `repo`: the local branch
/// `origin/HEAD` points at, else `main`, else `master`, else the
/// checked-out branch. None on a detached checkout with none of those.
/// One git call, which must answer by `deadline`.
async fn default_branch(
    transport: &dyn Transport,
    repo: &str,
    deadline: Deadline,
) -> Result<Option<String>, VcsError> {
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
    let output = query(transport, repo, &args, deadline).await?;
    let output = checked(output, "git could not list the branches")?;
    Ok(pick_default_branch(&output.stdout))
}

/// Picks the default branch from `git_default_branch`'s `for-each-ref`
/// output.
fn pick_default_branch(lines: &[String]) -> Option<String> {
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
    // `origin/HEAD` counts only when the branch it names exists locally:
    // a workspace starts from a local branch.
    let local = |name: &str| names.iter().any(|n| n == name);
    origin_head
        .filter(|name| local(name))
        .or_else(|| {
            ["main", "master"]
                .into_iter()
                .find(|n| local(n))
                .map(str::to_string)
        })
        .or(checked_out)
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
    resolve_commit(transport, repo, revision, deadline()).await
}

/// [`git_resolve_commit`], answering by `deadline`.
async fn resolve_commit(
    transport: &dyn Transport,
    repo: &str,
    revision: &str,
    deadline: Deadline,
) -> Result<Option<String>, VcsError> {
    // `^{commit}` peels a tag to the commit it tags and refuses anything
    // that is not a commit (a tree, a blob).
    let peeled = format!("{revision}^{{commit}}");
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

/// When the git queries for one answer must all have finished, and the
/// limit that was set from, to name in the error. `Copy`, so each query
/// takes it by value.
#[derive(Debug, Clone, Copy)]
struct Deadline {
    at: tokio::time::Instant,
    limit: Duration,
}

impl Deadline {
    /// A deadline `limit` from now.
    fn after(limit: Duration) -> Deadline {
        Deadline {
            at: tokio::time::Instant::now() + limit,
            limit,
        }
    }

    /// The error for running out of time.
    fn expired(&self, detail: String) -> VcsError {
        VcsError::Failed {
            message: format!("git did not answer within {} seconds", self.limit.as_secs()),
            stderr: detail,
        }
    }
}

/// The deadline for a quick query, [`GIT_QUERY_TIMEOUT`] from now.
fn deadline() -> Deadline {
    Deadline::after(GIT_QUERY_TIMEOUT)
}

/// Runs a git query that must answer by `deadline`, which several queries
/// for one answer share, so together they stay within its limit. Running
/// out of time is git's failure, reported as such: a query changes nothing,
/// so there is nothing half done to clean up.
async fn query(
    transport: &dyn Transport,
    dir: &str,
    args: &[&str],
    deadline: Deadline,
) -> Result<Output, VcsError> {
    query_env(transport, dir, args, &[], deadline).await
}

/// [`query`], with `env` added to git's environment.
async fn query_env(
    transport: &dyn Transport,
    dir: &str,
    args: &[&str],
    env: &[(&str, &str)],
    deadline: Deadline,
) -> Result<Output, VcsError> {
    let left = deadline
        .at
        .saturating_duration_since(tokio::time::Instant::now());
    match git_within(transport, dir, args, env, left).await {
        Err(VcsError::Incomplete(e)) if e.kind() == std::io::ErrorKind::TimedOut => {
            Err(deadline.expired(e.to_string()))
        }
        other => other,
    }
}

// -- What deleting a workspace would lose ---------------------------------

/// How long counting a workspace's changes may take, all its git calls and
/// file reads together. Longer than a quick query: it lists every untracked
/// and ignored file and reads every uncommitted one.
const CHANGES_TIMEOUT: Duration = Duration::from_secs(60);

/// How many commit ids one `git rev-list` is given on its command line.
/// Windows caps a command line at 32,767 characters; 200 ids of 40
/// characters stay well inside it.
const REVISIONS_PER_CALL: usize = 200;

/// What deleting an isolated workspace would lose: its folder's files, and
/// commits nothing but the workspace holds (PROTO-CORE-API-R48, and R46 when git has lost its record).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeChanges {
    /// Files with changes not committed, untracked files included.
    pub uncommitted: u64,
    /// Ignored entries, a wholly ignored folder as one.
    pub ignored: u64,
    /// Commits reachable from the checkout, the branch or the checkout's
    /// reflog that no other local or remote-tracking branch holds.
    pub unshared: u64,
    /// Those of them the branch does not hold.
    pub stranded: u64,
    /// git has no record of the worktree: its checkout and reflog are gone,
    /// so commits only they held can't be counted.
    pub unrecorded: bool,
    /// One line per thing that went into the counts, and the content of
    /// each uncommitted file: what the token is a digest of, so the token
    /// changes when any of them does.
    pub evidence: Vec<String>,
}

/// What deleting the git worktree at `path`, on branch `branch`, of the
/// repository at `repo` would lose. All of it is given [`CHANGES_TIMEOUT`].
///
/// git's own records of a worktree (its checkout, index and reflog, kept
/// in the repository) outlive its folder, so this answers for a worktree
/// whose removal has begun: with the folder partly gone, the files left are
/// counted; with it wholly gone, only commits. A removal that failed on
/// Windows can leave files git could not delete after git deleted its
/// record: those are counted as described at [`count_files`].
pub async fn git_worktree_changes(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    branch: &str,
) -> Result<WorktreeChanges, VcsError> {
    let deadline = Deadline::after(CHANGES_TIMEOUT);
    let mut changes = WorktreeChanges::default();
    let records = git_records(transport, repo, path, deadline).await?;
    changes.unrecorded = records.worktree.is_none();
    if changes.unrecorded {
        changes.evidence.push("unrecorded".to_string());
    }
    let folder = path.to_string();
    // `try_exists`, not `exists`: a folder Core may not look into is an
    // error, never a folder that is gone, whose files would go uncounted.
    let folder_exists = blocking(deadline, "looking for the workspace's folder", move |_| {
        Ok(std::path::Path::new(&folder).try_exists()?)
    })
    .await?;
    if folder_exists {
        count_files(transport, repo, &records, path, deadline, &mut changes).await?;
    }
    count_commits(
        transport,
        repo,
        records.worktree.as_deref(),
        branch,
        deadline,
        &mut changes,
    )
    .await?;
    Ok(changes)
}

/// Where git keeps a repository's records.
struct GitRecords {
    /// The repository's own git folder, shared by all its worktrees.
    common: String,
    /// The folder of git's records of one worktree
    /// (`<common>/worktrees/<name>`), or `None` when git has none.
    worktree: Option<String>,
}

/// The repository's git folder, and git's records of the worktree at
/// `path`.
///
/// The worktree's records are found by reading each record's `gitdir` file,
/// which names the worktree's folder and outlives it. Like [`find`], this
/// reads the local file system: worktrees are on the local host until
/// remote hosts (M4).
async fn git_records(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    deadline: Deadline,
) -> Result<GitRecords, VcsError> {
    let output = query(
        transport,
        repo,
        &["rev-parse", "--git-common-dir"],
        deadline,
    )
    .await?;
    let common = match output.stdout.first() {
        Some(line) if output.exit.success() => line.trim().to_string(),
        _ => {
            return Err(VcsError::Failed {
                message: "git could not find the repository's folder".into(),
                stderr: output.stderr_text(),
            });
        }
    };
    // git prints the folder relative to `repo` when it is inside it (`.git`);
    // joining an absolute path replaces `repo`.
    let common = std::path::Path::new(repo).join(common);
    let records = common.join("worktrees");
    let path = path.to_string();
    let worktree = blocking(
        deadline,
        "finding git's records of the worktree",
        move |stop| {
            let key = location_key(&path);
            // Anything but a missing file is an error, never "no record": a
            // record missed would leave its checkout's commits uncounted.
            let entries = match std::fs::read_dir(&records) {
                // No `worktrees` folder: git has no record of any worktree.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                other => other?,
            };
            for entry in entries {
                // The walk stops itself at the deadline, as the file reads do.
                if std::time::Instant::now() >= stop {
                    return Err(Halt::Expired);
                }
                let admin = entry?.path();
                // `gitdir` holds `<worktree folder>/.git`, relative to the
                // record's own folder when git writes relative paths
                // (`worktree.useRelativePaths`); joining an absolute one
                // replaces the record's folder.
                let gitdir = match std::fs::read_to_string(admin.join("gitdir")) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    other => other?,
                };
                let gitdir = without_dots(&admin.join(gitdir.trim()));
                let matches = gitdir
                    .parent()
                    .is_some_and(|folder| location_key(&folder.display().to_string()) == key);
                if matches {
                    return Ok(Some(admin.display().to_string()));
                }
            }
            Ok(None)
        },
    )
    .await?;
    Ok(GitRecords {
        common: common.display().to_string(),
        worktree,
    })
}

/// Why blocking file system work stopped early.
enum Halt {
    /// Its deadline passed; it stopped itself.
    Expired,
    /// The file system refused.
    Io(std::io::Error),
}

impl From<std::io::Error> for Halt {
    fn from(e: std::io::Error) -> Self {
        Halt::Io(e)
    }
}

/// The most blocking file system jobs `workspace.changes` and a delete's
/// folder removal may have running at once, Core-wide, as
/// `attachment.inspect` caps its reads. A job
/// answered at its deadline keeps its place until its thread returns, so a
/// folder that hangs, asked about again and again, takes this many threads,
/// never all of Core's.
static FILE_SYSTEM: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

/// Runs `work` on a blocking thread, answering by `deadline`; `what` names
/// it in errors. `work` gets the deadline as a clock time, to stop itself
/// between reads.
async fn blocking<T: Send + 'static>(
    deadline: Deadline,
    what: &'static str,
    work: impl FnOnce(std::time::Instant) -> Result<T, Halt> + Send + 'static,
) -> Result<T, VcsError> {
    let place = FILE_SYSTEM.try_acquire().map_err(|_| VcsError::Busy)?;
    let stop = deadline.at.into_std();
    let job = tokio::task::spawn_blocking(move || {
        let _place = place;
        work(stop)
    });
    let failed = |detail: String| VcsError::Failed {
        message: format!("{what} failed"),
        stderr: detail,
    };
    match tokio::time::timeout_at(deadline.at, job).await {
        Err(_) | Ok(Ok(Err(Halt::Expired))) => Err(VcsError::Failed {
            message: format!(
                "{what} did not finish within {} seconds",
                deadline.limit.as_secs()
            ),
            stderr: String::new(),
        }),
        Ok(Ok(Err(Halt::Io(e)))) => Err(failed(e.to_string())),
        Ok(Err(panicked)) => Err(failed(panicked.to_string())),
        Ok(Ok(Ok(value))) => Ok(value),
    }
}

/// `path` with its `.` and `..` resolved by their text alone, so it compares
/// equal to the folder it names when that folder no longer exists to be
/// canonicalised.
fn without_dots(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Counts the uncommitted files and the ignored entries in the worktree's
/// folder, and records them, and each uncommitted file's content, as
/// evidence.
///
/// Without git's records of the worktree, git cannot say which files left
/// in the folder were committed: each is judged against an empty index, so
/// every file left counts as uncommitted, and the ignore rules still apply.
async fn count_files(
    transport: &dyn Transport,
    repo: &str,
    records: &GitRecords,
    path: &str,
    deadline: Deadline,
    changes: &mut WorktreeChanges,
) -> Result<(), VcsError> {
    // Naming git's records and the folder outright, rather than running in
    // the folder, works when the folder's `.git` file is gone, and never
    // finds some other repository above a half-deleted folder.
    let git_dir = format!(
        "--git-dir={}",
        records.worktree.as_deref().unwrap_or(&records.common)
    );
    let work_tree = format!("--work-tree={path}");
    // An index file that does not exist reads as an empty index. A path
    // nobody else uses, never created: `--no-optional-locks` keeps git from
    // writing an index. Local, like the records' lookup, until M4.
    let empty_index = std::env::temp_dir()
        .join(format!("ersa-empty-index-{}", uuid::Uuid::new_v4()))
        .display()
        .to_string();
    let env: &[(&str, &str)] = match records.worktree {
        Some(_) => &[],
        None => &[("GIT_INDEX_FILE", empty_index.as_str())],
    };
    // `--no-optional-locks`: otherwise `status` may refresh the index,
    // writing to the workspace while a session works in it. `-z` ends each
    // entry with NUL and never quotes a path.
    let status = |untracked: &'static str, ignored: bool| {
        let mut args = vec![
            "--no-optional-locks",
            git_dir.as_str(),
            work_tree.as_str(),
            "status",
            "--porcelain=v2",
            "-z",
            untracked,
        ];
        if ignored {
            args.push("--ignored");
        }
        args
    };

    // Every untracked file, each its own entry.
    let args = status("--untracked-files=all", false);
    let output = query_env(transport, repo, &args, env, deadline).await?;
    let entries = parse_status(&checked(output, "git status failed")?.stdout);

    // Submodules: the index's entries of mode 160000. A clean one is not in
    // `status`, yet commits made in it may be held nowhere else. Without
    // git's records there is no index: a submodule's folder is then listed
    // as an untracked nested repository.
    let submodules = match records.worktree {
        Some(_) => {
            let args = [
                git_dir.as_str(),
                work_tree.as_str(),
                "ls-files",
                "--stage",
                "-z",
            ];
            let output = query(transport, repo, &args, deadline).await?;
            submodule_paths(&checked(output, "git ls-files failed")?.stdout)
        }
        None => BTreeSet::new(),
    };

    let mut to_read = Vec::new();
    let mut repositories = submodules.clone();
    let mut counted = BTreeSet::new();
    for entry in entries {
        // Without git's records, the staged deletions are the repository's
        // own checkout measured against an empty index: they say nothing
        // about the folder, so they stay out of the token too.
        if records.worktree.is_some() || entry.kind == '?' {
            changes.evidence.push(format!("status {}", entry.raw));
        }
        if entry.gone_from_folder() {
            continue;
        }
        // `-uall` lists a folder only when it is a nested repository; it is
        // judged as one, below, like a submodule.
        if entry.kind == '?' && entry.path.ends_with('/') {
            repositories.insert(entry.path.trim_end_matches('/').to_string());
            continue;
        }
        if submodules.contains(&entry.path) {
            // A submodule `status` lists has changed against the commit,
            // which counts it; it is also looked into, below, for the token.
            changes.uncommitted += 1;
            counted.insert(entry.path);
            continue;
        }
        changes.uncommitted += 1;
        if entry.folder_differs() {
            to_read.push(entry.path);
        }
    }

    // Each submodule and nested repository: its HEAD and status go into the
    // token, and one with work of its own counts once.
    for relative in &repositories {
        let dir = std::path::Path::new(path).join(relative);
        let Some(nested) = inspect_repository(transport, &dir, deadline).await? else {
            continue;
        };
        changes
            .evidence
            .push(format!("repository {relative} {}", nested.evidence));
        if nested.has_work && !counted.contains(relative) {
            changes.uncommitted += 1;
        }
    }

    // The ignored entries, with a wholly ignored folder as one: `--ignored`
    // collapses one only while untracked folders are collapsed too, hence a
    // second call.
    let args = status("--untracked-files=normal", true);
    let output = query_env(transport, repo, &args, env, deadline).await?;
    for entry in parse_status(&checked(output, "git status failed")?.stdout) {
        if entry.kind == '!' {
            changes.ignored += 1;
            changes.evidence.push(format!("ignored {}", entry.path));
        }
    }

    // The content of each uncommitted file, so editing one changes the
    // token. Read here, not by git: git cannot hash a link to a folder, and
    // fails outright on a file deleted since it listed it. The reading
    // stops itself at the deadline, so a huge file is not read on after
    // the answer has gone; only a read the file system never returns from
    // keeps its thread.
    let folder = std::path::PathBuf::from(path);
    let contents = blocking(deadline, "reading the uncommitted files", move |stop| {
        let mut contents = Vec::with_capacity(to_read.len());
        for relative in to_read {
            let digest = content_digest(&folder.join(&relative), stop).ok_or(Halt::Expired)?;
            contents.push(format!("content {digest} {relative}"));
        }
        Ok(contents)
    })
    .await?;
    changes.evidence.extend(contents);
    Ok(())
}

/// The paths of the submodules in `git ls-files --stage -z` output: the
/// entries of mode 160000, each `<mode> <object> <stage>` and a tab before the path.
fn submodule_paths(lines: &[String]) -> BTreeSet<String> {
    lines
        .join("\n")
        .split('\0')
        .filter_map(|entry| entry.split_once('\t'))
        .filter(|(stage, _)| stage.starts_with("160000 "))
        .map(|(_, path)| path.to_string())
        .collect()
}

/// What deleting a submodule's or nested repository's folder would lose.
struct Repository {
    /// Uncommitted changes, or commits none of its branches' remotes hold.
    has_work: bool,
    /// Its HEAD, a digest of its status, and whether it has such commits.
    evidence: String,
}

/// Looks into the submodule or nested repository at `dir`; `None` when it
/// has no `.git` of its own (a submodule never checked out), which holds
/// nothing, and where git would find the workspace's repository instead.
async fn inspect_repository(
    transport: &dyn Transport,
    dir: &std::path::Path,
    deadline: Deadline,
) -> Result<Option<Repository>, VcsError> {
    let marker = dir.join(".git");
    let found = blocking(deadline, "looking into a nested repository", move |_| {
        Ok(marker.try_exists()?)
    })
    .await?;
    if !found {
        return Ok(None);
    }
    let dir = dir.display().to_string();

    let head = ["rev-parse", "--verify", "--quiet", "HEAD"];
    let output = query(transport, &dir, &head, deadline).await?;
    // Exit 1: no commit yet.
    let head = if output.exit.code == Some(1) {
        None
    } else {
        let output = checked(output, "git could not read a nested repository's checkout")?;
        commit_ids(&output.stdout).next()
    };

    let status = [
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
    ];
    let output = query(transport, &dir, &status, deadline).await?;
    let output = checked(output, "git status failed in a nested repository")?;
    let entries = parse_status(&output.stdout);
    let dirty = entries.iter().any(|entry| entry.kind != '!');
    let mut digest = Digest::default();
    for entry in &entries {
        digest.add(&entry.raw);
    }

    // Its checkout and branches, less what its remote-tracking branches
    // hold: one commit is enough to know.
    let mut args = vec!["rev-list", "--max-count=1"];
    args.extend(head.as_deref());
    args.extend(["--branches", "--not", "--remotes"]);
    let output = query(transport, &dir, &args, deadline).await?;
    let output = checked(output, "git could not list a nested repository's commits")?;
    let unshared = commit_ids(&output.stdout).next().is_some();

    Ok(Some(Repository {
        has_work: dirty || unshared,
        evidence: format!(
            "{} {} {unshared}",
            head.as_deref().unwrap_or("none"),
            digest.finish()
        ),
    }))
}

/// One entry of `git status --porcelain=v2 -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusEntry {
    /// `1` changed, `2` renamed or copied, `u` unmerged, `?` untracked,
    /// `!` ignored.
    kind: char,
    /// The index's and the folder's state against the commit, such as `.M`
    /// or `A.`; `..` for untracked and ignored entries.
    xy: String,
    /// Relative to the worktree's root, with `/` separators.
    path: String,
    /// The whole entry, a renamed file's original path included.
    raw: String,
}

impl StatusEntry {
    /// Whether deleting the folder loses nothing of the file: it is gone
    /// from the folder, and the index holds nothing the commit does not
    /// (deleted from the folder alone, or the deletion staged). A file
    /// staged and then deleted from the folder still counts: the index is
    /// deleted with the workspace.
    fn gone_from_folder(&self) -> bool {
        matches!(self.kind, '1' | '2') && matches!(self.xy.as_str(), ".D" | "D.")
    }

    /// Whether the folder's copy may differ from the index's, which git's
    /// entry does not identify by content.
    fn folder_differs(&self) -> bool {
        match self.kind {
            '1' | '2' => !self.xy.ends_with('.'),
            _ => true,
        }
    }
}

/// Parses `git status --porcelain=v2 -z`. Each entry ends in NUL, and a
/// renamed entry is followed by its original path; fields are separated by
/// spaces, with the path last, so a path may hold spaces.
fn parse_status(lines: &[String]) -> Vec<StatusEntry> {
    // The transport splits output into lines; with `-z` the only newlines
    // are inside paths, so joining the lines restores the output.
    let text = lines.join("\n");
    let mut fields = text.split('\0');
    let mut entries = Vec::new();
    while let Some(field) = fields.next() {
        let Some(kind) = field.chars().next() else {
            continue;
        };
        // The number of fields before the path.
        let before_path = match kind {
            '1' => 8,
            '2' => 9,
            'u' => 10,
            '?' | '!' => 1,
            _ => continue,
        };
        let Some(path) = field.splitn(before_path + 1, ' ').nth(before_path) else {
            continue;
        };
        let mut raw = field.to_string();
        if kind == '2' {
            raw.push('\0');
            raw.push_str(fields.next().unwrap_or_default());
        }
        let xy = match kind {
            '1' | '2' | 'u' => field.get(2..4).unwrap_or_default().to_string(),
            _ => "..".to_string(),
        };
        entries.push(StatusEntry {
            kind,
            xy,
            path: path.to_string(),
            raw,
        });
    }
    entries
}

/// The SHA-256 of a file's content or a link's target, or what is known of
/// what is there instead; `None` once `stop` passes. Blocking.
fn content_digest(file: &std::path::Path, stop: std::time::Instant) -> Option<String> {
    use std::io::Read as _;

    use sha2::Digest as _;
    let metadata = match std::fs::symlink_metadata(file) {
        Ok(metadata) => metadata,
        Err(e) => return Some(format!("unreadable:{:?}", e.kind())),
    };
    // What a file that cannot be read is known by: its size and the time it
    // was last written, which change when its content does.
    let unreadable = |e: std::io::Error| {
        let written = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        format!("unreadable:{:?}:{}:{written}", e.kind(), metadata.len())
    };
    if metadata.file_type().is_symlink() {
        return Some(match std::fs::read_link(file) {
            Ok(target) => format!(
                "link:{}",
                hex(&sha2::Sha256::digest(target.display().to_string()))
            ),
            Err(e) => unreadable(e),
        });
    }
    if metadata.is_dir() {
        // A nested repository: git lists it, not the files in it.
        return Some("folder".to_string());
    }
    let mut file = match std::fs::File::open(file) {
        Ok(file) => file,
        Err(e) => return Some(unreadable(e)),
    };
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        if std::time::Instant::now() >= stop {
            return None;
        }
        match file.read(&mut buffer) {
            Ok(0) => return Some(hex(&hasher.finalize())),
            Ok(n) => hasher.update(&buffer[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Some(unreadable(e)),
        }
    }
}

/// Counts the commits only the workspace holds, and records them as
/// evidence. `admin` is git's record of the worktree, when it has one,
/// for its checkout and reflog.
async fn count_commits(
    transport: &dyn Transport,
    repo: &str,
    admin: Option<&str>,
    branch: &str,
    deadline: Deadline,
    changes: &mut WorktreeChanges,
) -> Result<(), VcsError> {
    // The checkout and every commit its reflog names: a detached checkout's
    // commits, and those a reset left behind, are held by nothing else.
    let mut tips = BTreeSet::new();
    if let Some(admin) = admin {
        let git_dir = format!("--git-dir={admin}");
        let head = [git_dir.as_str(), "rev-parse", "--verify", "--quiet", "HEAD"];
        let output = query(transport, repo, &head, deadline).await?;
        // `--verify --quiet` exits 1 for a HEAD with no commit yet, which
        // has no reflog either; any other failure is git's.
        if output.exit.code != Some(1) {
            let output = checked(output, "git could not read the workspace's checkout")?;
            tips.extend(commit_ids(&output.stdout));
            let reflog = [git_dir.as_str(), "reflog", "show", "--format=%H", "HEAD"];
            let output = query(transport, repo, &reflog, deadline).await?;
            let output = checked(output, "git could not read the reflog")?;
            tips.extend(commit_ids(&output.stdout));
        }
    }

    // What the branch holds that no other branch does; a branch already
    // deleted holds nothing.
    let on_branch = if branch.is_empty() {
        BTreeSet::new()
    } else {
        let tip = [format!("refs/heads/{branch}")];
        unshared_from(transport, repo, &tip, branch, deadline).await?
    };
    let tips: Vec<String> = tips.into_iter().collect();
    let mut stranded = BTreeSet::new();
    for batch in tips.chunks(REVISIONS_PER_CALL) {
        stranded.extend(unshared_from(transport, repo, batch, branch, deadline).await?);
    }
    stranded.retain(|commit| !on_branch.contains(commit));

    changes.unshared = (on_branch.len() + stranded.len()) as u64;
    changes.stranded = stranded.len() as u64;
    let on_branch = on_branch.iter().map(|c| format!("unshared {c}"));
    let off_branch = stranded.iter().map(|c| format!("stranded {c}"));
    changes.evidence.extend(on_branch.chain(off_branch));
    Ok(())
}

/// The commits reachable from `tips` that no local branch but `branch`,
/// and no remote-tracking branch, holds.
async fn unshared_from(
    transport: &dyn Transport,
    repo: &str,
    tips: &[String],
    branch: &str,
    deadline: Deadline,
) -> Result<BTreeSet<String>, VcsError> {
    // `--ignore-missing` skips a tip that no longer exists: a branch
    // deleted, or a reflog entry whose commit git has since collected.
    let mut args = vec!["rev-list", "--ignore-missing"];
    args.extend(tips.iter().map(String::as_str));
    // Everything after `--not` is subtracted. `--exclude` leaves the
    // workspace's own branch out of the `--branches` that follows it; a
    // branch name can hold no glob character, so it matches only itself.
    args.push("--not");
    let exclude = format!("--exclude={branch}");
    if !branch.is_empty() {
        args.push(&exclude);
    }
    args.extend(["--branches", "--remotes"]);
    let output = query(transport, repo, &args, deadline).await?;
    let output = checked(output, "git could not list the workspace's commits")?;
    Ok(commit_ids(&output.stdout).collect())
}

/// The commit ids in git's output, one per line.
fn commit_ids(lines: &[String]) -> impl Iterator<Item = String> + '_ {
    lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

/// `output` when git succeeded, else its failure with `message`.
fn checked(output: Output, message: &str) -> Result<Output, VcsError> {
    if output.exit.success() {
        Ok(output)
    } else {
        Err(VcsError::Failed {
            message: message.into(),
            stderr: output.stderr_text(),
        })
    }
}

/// A SHA-256 digest of a sequence of strings, for an opaque token. Each is
/// added with its length first, so no two sequences give the same input.
#[derive(Default)]
pub struct Digest(sha2::Sha256);

impl Digest {
    pub fn add(&mut self, item: &str) {
        use sha2::Digest as _;
        self.0.update((item.len() as u64).to_le_bytes());
        self.0.update(item.as_bytes());
    }

    /// The digest, in lowercase hexadecimal.
    pub fn finish(self) -> String {
        use sha2::Digest as _;
        hex(&self.0.finalize())
    }
}

/// `bytes` in lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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
    let args = &args;
    retried(
        || async move {
            let output = git(transport, repo, args).await?;
            Ok(if output.exit.success() {
                Ok(())
            } else {
                Err(output.stderr_text())
            })
        },
        |stderr| VcsError::Failed {
            message: "git worktree remove failed".into(),
            stderr,
        },
    )
    .await
}

/// Runs `once` up to [`REMOVE_ATTEMPTS`] times, waiting
/// [`REMOVE_FIRST_BACKOFF`], doubled each time, between tries. `once`
/// answers `Ok(Ok(()))` when the removal is done, `Ok(Err(why))` for a try
/// worth repeating, and `Err` to end at once; the last try's `why` becomes
/// the error `failed` makes.
async fn retried<Try>(
    mut once: impl FnMut() -> Try,
    failed: impl FnOnce(String) -> VcsError,
) -> Result<(), VcsError>
where
    Try: std::future::Future<Output = Result<Result<(), String>, VcsError>>,
{
    let mut backoff = REMOVE_FIRST_BACKOFF;
    let mut attempt = 1;
    loop {
        let why = match once().await? {
            Ok(()) => return Ok(()),
            Err(why) => why,
        };
        if attempt == REMOVE_ATTEMPTS {
            return Err(failed(why));
        }
        tracing::debug!(attempt, %why, "removal failed; retrying");
        tokio::time::sleep(backoff).await;
        backoff *= 2;
        attempt += 1;
    }
}

/// As [`retried`], but tries go on until `window` after the first try
/// began, rather than a fixed number of times (PROTO-CORE-API-R113). The
/// waits start at [`REMOVE_FIRST_BACKOFF`] and double up to
/// [`FOLDER_TRY_LONGEST_WAIT`]; a wait that would end past the window is cut
/// short, so the last try begins at the window's end, never after it.
async fn retried_within<Try>(
    window: Duration,
    mut once: impl FnMut() -> Try,
    failed: impl FnOnce(String) -> VcsError,
) -> Result<(), VcsError>
where
    Try: std::future::Future<Output = Result<Result<(), String>, VcsError>>,
{
    let last_start = tokio::time::Instant::now() + window;
    let mut backoff = REMOVE_FIRST_BACKOFF;
    let mut attempt = 1;
    loop {
        // `Err` (a try that reached its time limit among them) ends at once.
        let why = match once().await? {
            Ok(()) => return Ok(()),
            Err(why) => why,
        };
        let now = tokio::time::Instant::now();
        if now >= last_start {
            return Err(failed(why));
        }
        tracing::debug!(attempt, %why, "removal failed; retrying");
        tokio::time::sleep_until((now + backoff).min(last_start)).await;
        backoff = (backoff * 2).min(FOLDER_TRY_LONGEST_WAIT);
        attempt += 1;
    }
}

/// Deletes an isolated workspace's files for the user's Delete
/// (PROTO-CORE-API-R35): the git worktree at `path` of the repository at
/// `repo`, forced, since the user was shown what it held, and then its
/// branch `branch` unless another worktree has it checked out or it holds
/// commits no other local or remote-tracking branch holds. Returns whether
/// the branch was kept.
///
/// A folder git no longer lists (on Windows a failed removal can delete
/// git's record and leave files behind) is deleted as a folder; a locked
/// worktree is refused, as `workspace.remove` refuses it. Safe to run
/// again after it failed part way: a folder already gone is pruned, and a
/// branch already gone is not kept.
///
/// `tries` says how each try at a folder git no longer lists runs
/// ([`remove_folder`]). With [`FolderTries::window`] set (`session.delete`),
/// a git removal that fails after git dropped its record of the worktree
/// goes on to delete the folder in this same call (PROTO-CORE-API-R112).
/// Every failure of git's removal is logged as a warning
/// (PROTO-CORE-API-R114); the caller's span names the session.
///
/// git's part runs in the repository's turns (`worktrees`): its removal in
/// one, its prune and the branch in another. The tries at the folder, which
/// can last [`FOLDER_TRY_WINDOW`] and touch only the folder, run between
/// them, holding up no other change to the repository's worktrees.
pub async fn git_worktree_delete(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
    branch: &str,
    tries: FolderTries,
    worktrees: &dyn Worktrees,
) -> Result<bool, VcsError> {
    // Kept, not read: holding it is the turn.
    let mut _turn = worktrees.turn().await?;
    let as_folder = match git_worktree_remove(transport, repo, path, true, false).await {
        Ok(()) => false,
        Err(VcsError::NotListed { .. }) => true,
        Err(e) => {
            tracing::warn!(path, reason = %describe(&e), "git's removal of the worktree failed");
            // On Windows git can fail on a file in use after deleting its
            // record of the worktree, leaving the files: its own tries then
            // fail on a worktree it no longer lists. Only a removal that ran
            // and failed counts; one that timed out was called off (R91).
            let gone_from_git = tries.window.is_some()
                && matches!(e, VcsError::Failed { .. })
                && matches!(
                    git_worktree_list(transport, repo).await,
                    Ok(listed) if find(&listed, path).await.is_none()
                );
            if !gone_from_git {
                return Err(e);
            }
            true
        }
    };
    if as_folder {
        drop(_turn);
        remove_folder(path, &tries).await?;
        _turn = worktrees.turn().await?;
        let pruned = git(transport, repo, &["worktree", "prune"]).await?;
        check(pruned, "git worktree prune failed")?;
    }
    if branch.is_empty() {
        return Ok(false);
    }
    let deadline = deadline();
    let reference = format!("refs/heads/{branch}");
    let exists = ["rev-parse", "--verify", "--quiet", reference.as_str()];
    let output = query(transport, repo, &exists, deadline).await?;
    match output.exit.code {
        Some(0) => {}
        // Gone already: deleted by an earlier try, or by the user.
        Some(1) => return Ok(false),
        _ => {
            return Err(VcsError::Failed {
                message: "git could not look up the workspace's branch".into(),
                stderr: output.stderr_text(),
            });
        }
    }
    // Checked out in another worktree: git would refuse to delete it, and
    // the other worktree is using it.
    let listed = git_worktree_list(transport, repo).await?;
    if listed
        .iter()
        .any(|entry| entry.branch.as_deref() == Some(reference.as_str()))
    {
        return Ok(true);
    }
    // Commits only the branch holds are kept with it.
    let unshared = unshared_from(
        transport,
        repo,
        std::slice::from_ref(&reference),
        branch,
        deadline,
    )
    .await?;
    if !unshared.is_empty() {
        return Ok(true);
    }
    // `-D`: every commit on it is held elsewhere, which `-d` would check
    // only against the checked-out branch.
    let deleted = git(transport, repo, &["branch", "-D", branch]).await?;
    check(deleted, "git could not delete the workspace's branch")?;
    Ok(false)
}

/// Whether the git worktree at `path` of the repository at `repo` is gone:
/// git no longer lists it (it keeps a worktree's record, prunable, until it
/// is removed or pruned, even with its folder gone) and nothing is at
/// `path`. The look at the folder shares `workspace.changes`'s cap on
/// blocking reads and is given [`GIT_QUERY_TIMEOUT`].
pub async fn git_worktree_gone(
    transport: &dyn Transport,
    repo: &str,
    path: &str,
) -> Result<bool, VcsError> {
    let listed = git_worktree_list(transport, repo).await?;
    if find(&listed, path).await.is_some() {
        return Ok(false);
    }
    let folder = path.to_string();
    let exists = blocking(
        deadline(),
        "looking for the workspace's folder",
        move |_| Ok(std::path::Path::new(&folder).try_exists()?),
    )
    .await?;
    Ok(!exists)
}

/// How [`git_worktree_delete`]'s tries at a workspace's folder run.
#[derive(Clone)]
pub struct FolderTries {
    /// Kept by each try on its thread until the thread returns, which a
    /// file system call that hangs delays past the answer: a delete's
    /// claim, so nothing begins a second removal beside one still stuck.
    pub held: Arc<dyn std::any::Any + Send + Sync>,
    /// How long each try may take: [`GIT_TIMEOUT`], as a git call has,
    /// except in tests that let it run out.
    pub limit: Duration,
    /// Where each try stops, on its thread before it touches the folder,
    /// until a test lets it go ([`crate::CoreConfig::folder_gate`]).
    pub gate: Option<Arc<Gate>>,
    /// `session.delete`'s step 5 sets this to how long tries at the folder
    /// go on ([`FOLDER_TRY_WINDOW`], PROTO-CORE-API-R113), and then a failed
    /// git removal of a worktree git no longer lists goes on to the folder
    /// (PROTO-CORE-API-R112). `None` elsewhere (fork cleanup): the folder
    /// gets [`REMOVE_ATTEMPTS`] tries, and git's failure ends the call.
    pub window: Option<Duration>,
}

impl FolderTries {
    /// Tries that hold `held`, each given [`GIT_TIMEOUT`].
    pub fn holding(held: Arc<dyn std::any::Any + Send + Sync>) -> FolderTries {
        FolderTries {
            held,
            limit: GIT_TIMEOUT,
            gate: None,
            window: None,
        }
    }
}

/// Deletes the folder at `path` and everything in it, retried while files
/// are busy, as worktree removal is. A link inside is removed, never
/// followed. A folder already gone is not an error. Each try is a blocking
/// job under [`FILE_SYSTEM`]'s cap, given `tries.limit`, so a folder on a
/// drive that stops answering fails the delete rather than hold it for
/// ever, and keeps `tries.held` until its thread returns. Once Core gives
/// up on a try, at its deadline or because the request was dropped, the try
/// begins no further deletion (PROTO-CORE-API-R91): it asks before each one.
async fn remove_folder(path: &str, tries: &FolderTries) -> Result<(), VcsError> {
    let call_off = CallOff::default();
    let _called_off_on_return = CallOffOnDrop(call_off.clone());
    let call_off = &call_off;
    let runtime = &tokio::runtime::Handle::current();
    let once = || async move {
        let folder = std::path::PathBuf::from(path);
        let call_off = call_off.clone();
        let (held, gate) = (Arc::clone(&tries.held), tries.gate.clone());
        let runtime = runtime.clone();
        let deadline = Deadline::after(tries.limit);
        blocking(deadline, "deleting the workspace's folder", move |at| {
            let _held = held;
            // In tests, the folder that does not answer.
            if let Some(gate) = gate {
                runtime.block_on(gate.stop());
            }
            let stop = || call_off.is_called_off() || std::time::Instant::now() >= at;
            match remove_tree(&folder, &stop) {
                Ok(Removal::Removed) => Ok(Ok(())),
                Ok(Removal::CalledOff) => Err(Halt::Expired),
                // Busy files are worth another try.
                Err(e) => Ok(Err(e.to_string())),
            }
        })
        .await
    };
    let failed = |stderr| VcsError::Failed {
        message: format!("the workspace's folder {path} could not be deleted"),
        stderr,
    };
    match tries.window {
        Some(window) => retried_within(window, once, failed).await,
        None => retried(once, failed).await,
    }
}

/// `e` with git's own words, for the diagnostic log.
fn describe(e: &VcsError) -> String {
    match e {
        VcsError::Failed { message, stderr } if !stderr.is_empty() => {
            format!("{message}: {}", stderr.trim())
        }
        _ => e.to_string(),
    }
}

/// Whether the worktree at `path` has anything `git worktree remove` would
/// refuse over: modified or untracked files. Ignored files do not count.
async fn is_dirty(transport: &dyn Transport, path: &str) -> Result<bool, VcsError> {
    let output = git(transport, path, &["status", "--porcelain"]).await?;
    let output = checked(output, "git status failed")?;
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
    /// The branch checked out there (`refs/heads/<name>`); `None` when the
    /// checkout is detached.
    pub branch: Option<String>,
}

/// The repository's worktrees, the main one included.
pub async fn git_worktree_list(
    transport: &dyn Transport,
    repo: &str,
) -> Result<Vec<WorktreeEntry>, VcsError> {
    let output = git(transport, repo, &["worktree", "list", "--porcelain"]).await?;
    let output = checked(output, "git worktree list failed")?;
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
            "branch" => {
                if let Some(entry) = current.as_mut() {
                    entry.branch = value.map(str::to_string);
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
    git_within(transport, dir, args, &[], GIT_TIMEOUT).await
}

/// Runs `git -C <dir> <args>` with a fixed environment, killing it after
/// `timeout`.
async fn git_within(
    transport: &dyn Transport,
    dir: &str,
    args: &[&str],
    extra_env: &[(&str, &str)],
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
            .chain(extra_env)
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        // Which repository git reads is named by its arguments alone, never
        // by what Core inherited (a git hook exports `GIT_DIR`, for one).
        env_remove: [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_OBJECT_DIRECTORY",
        ]
        .map(String::from)
        .to_vec(),
        ..ProcessSpec::default()
    };
    Ok(transport::run(transport, spec, timeout).await?)
}

fn check(output: Output, message: &str) -> Result<(), VcsError> {
    checked(output, message).map(drop)
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
        assert_eq!(entries[1].branch.as_deref(), Some("refs/heads/a"));
        assert_eq!(entries[2].branch, None);
    }

    // verifies: PROTO-CORE-API-R19
    #[test]
    fn main_comes_before_master() {
        // The socket tests cover each rule alone; this is the one case they
        // don't: both exist, with another branch checked out.
        let text = "refs/heads/main: :\nrefs/heads/master: :\nrefs/heads/x:*:";
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        assert_eq!(pick_default_branch(&lines).as_deref(), Some("main"));
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

    // verifies: PROTO-CORE-API-R19
    #[tokio::test(start_paused = true)]
    async fn finding_the_default_branch_gives_git_ten_seconds() {
        let started = tokio::time::Instant::now();
        let result = git_default_commit(&hanging_git(), "repo").await;
        timed_out_at_ten_seconds(result, started);
    }

    /// Finding the default branch and resolving it share one limit, so a
    /// repository slow to list its branches does not get a second ten
    /// seconds to resolve one.
    // verifies: PROTO-CORE-API-R109
    #[tokio::test(start_paused = true)]
    async fn finding_and_resolving_the_default_branch_get_ten_seconds_together() {
        /// The first git lists `main` after six seconds; the next never answers.
        struct SlowThenHanging(std::sync::atomic::AtomicUsize);
        #[ersa_adapter::async_trait]
        impl Transport for SlowThenHanging {
            async fn spawn(
                &self,
                spec: ProcessSpec,
            ) -> std::io::Result<Box<dyn ersa_adapter::Process>> {
                if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    tokio::time::sleep(Duration::from_secs(6)).await;
                    let listing = ersa_adapter::fake::FakeTransport::new(["refs/heads/main:*:"]);
                    return listing.spawn(spec).await;
                }
                hanging_git().spawn(spec).await
            }
        }
        let transport = SlowThenHanging(Default::default());
        let started = tokio::time::Instant::now();
        let result = git_default_commit(&transport, "repo").await;
        assert_eq!(transport.0.load(std::sync::atomic::Ordering::SeqCst), 2);
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

    // verifies: PROTO-CORE-API-R48
    #[test]
    fn status_entries_keep_spaces_and_a_renamed_file_s_original_path() {
        // As the transport hands it over: `-z` output, split into lines at
        // the newline inside one path.
        let text = concat!(
            "1 .M N... 100644 100644 100644 aa bb with space.txt\0",
            "2 R. N... 100644 100644 100644 aa aa R100 new name\0old name\0",
            "? line\nbreak\0",
            "! build/\0",
        );
        let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let entries = parse_status(&lines);
        let paths: Vec<_> = entries.iter().map(|e| (e.kind, e.path.as_str())).collect();
        assert_eq!(
            paths,
            [
                ('1', "with space.txt"),
                ('2', "new name"),
                ('?', "line\nbreak"),
                ('!', "build/")
            ]
        );
        assert!(entries[1].raw.ends_with("\0old name"));
        assert!(entries[0].folder_differs() && !entries[1].folder_differs());
    }

    // verifies: PROTO-CORE-API-R47
    #[tokio::test(start_paused = true)]
    async fn counting_changes_gives_git_sixty_seconds_in_all() {
        let started = tokio::time::Instant::now();
        let result = git_worktree_changes(&hanging_git(), "repo", "folder", "branch").await;
        assert_eq!(started.elapsed(), Duration::from_secs(60));
        match result {
            Err(VcsError::Failed { message, .. }) => {
                assert!(message.contains("60 seconds"), "{message}")
            }
            other => panic!("expected a timed-out git failure, got {other:?}"),
        }
    }

    // verifies: PROTO-CORE-API-R47
    #[test]
    fn reading_a_file_stops_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("big");
        std::fs::write(&file, vec![7u8; 256 * 1024]).unwrap();
        let later = std::time::Instant::now() + Duration::from_secs(60);
        assert!(content_digest(&file, later).is_some());
        // A deadline already passed stops the read before its first chunk.
        assert_eq!(content_digest(&file, std::time::Instant::now()), None);
    }

    /// When each of `retried_within`'s tries began, measured from the first,
    /// and the error it ended with, for a folder whose every try fails.
    async fn tries_that_keep_failing(window: Duration) -> (Vec<Duration>, VcsError) {
        let first = tokio::time::Instant::now();
        let starts = std::sync::Mutex::new(Vec::new());
        let result = retried_within(
            window,
            || {
                let count = {
                    let mut starts = starts.lock().unwrap();
                    starts.push(first.elapsed());
                    starts.len()
                };
                async move { Ok(Err(format!("in use, try {count}"))) }
            },
            |stderr| VcsError::Failed {
                message: "could not be deleted".into(),
                stderr,
            },
        )
        .await;
        (starts.into_inner().unwrap(), result.unwrap_err())
    }

    /// A folder that stays in use is tried again for 30 seconds from the
    /// first try, none beginning later, and the delete fails with the last
    /// try's reason. The clock is paused, so the test does not wait.
    // verifies: PROTO-CORE-API-R113
    #[tokio::test(start_paused = true)]
    async fn tries_at_a_folder_go_on_for_30_seconds_and_no_longer() {
        let (starts, error) = tries_that_keep_failing(FOLDER_TRY_WINDOW).await;
        let last = *starts.last().unwrap();
        assert_eq!(last, Duration::from_secs(30), "{starts:?}");
        // Waits between them, each at most 2 seconds.
        for pair in starts.windows(2) {
            let wait = pair[1] - pair[0];
            assert!(
                wait > Duration::ZERO && wait <= Duration::from_secs(2),
                "{starts:?}"
            );
        }
        let VcsError::Failed { stderr, .. } = error else {
            panic!("{error:?}");
        };
        assert_eq!(stderr, format!("in use, try {}", starts.len()));
    }

    /// A try that ends in an error rather than a reason to try again, as a
    /// try that reached its time limit does, is not tried again.
    // verifies: PROTO-CORE-API-R113
    #[tokio::test(start_paused = true)]
    async fn a_try_at_a_folder_that_reached_its_limit_is_not_tried_again() {
        let tries = std::sync::atomic::AtomicU32::new(0);
        let result = retried_within(
            FOLDER_TRY_WINDOW,
            || {
                tries.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async {
                    Err(VcsError::Failed {
                        message: "deleting the workspace's folder did not finish".into(),
                        stderr: String::new(),
                    })
                }
            },
            |_| unreachable!("the error is the try's own"),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(tries.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// The cap's tests take its places, so they run one at a time.
    static CAP_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// A try at a folder that Core has given up on deletes nothing after:
    /// it asks before each deletion, and a deadline already passed stops it
    /// before the first. The runtime is dropped before the files are looked
    /// at, which waits for the try's thread to return.
    // verifies: PROTO-CORE-API-R91
    #[test]
    fn a_folder_removal_given_up_on_deletes_nothing_after() {
        let _one_at_a_time = CAP_TESTS.blocking_lock();
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("workspace");
        let files = ["a.txt", "b.txt", "sub/c.txt"];
        for name in files {
            let file = folder.join(name);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, name).unwrap();
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let path = folder.display().to_string();
        let tries = FolderTries {
            limit: Duration::ZERO,
            ..FolderTries::holding(Arc::new(()))
        };
        let result = runtime.block_on(remove_folder(&path, &tries));
        drop(runtime);
        let Err(VcsError::Failed { message, .. }) = result else {
            panic!("{result:?}");
        };
        assert_eq!(
            message,
            "deleting the workspace's folder did not finish within 0 seconds"
        );
        for name in files {
            assert!(folder.join(name).exists(), "{name}");
        }
    }

    /// A try at a folder whose file system call hangs past its limit keeps
    /// what it holds, a delete's claim, until the call returns, so a retry
    /// meanwhile is refused rather than start a second removal beside it
    /// (PROTO-CORE-API-R107).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_folder_removal_that_hangs_keeps_its_hold_until_its_thread_returns() {
        let _one_at_a_time = CAP_TESTS.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("workspace");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.txt"), "a").unwrap();
        let held = Arc::new(());
        // A folder the file system does not answer for until told to.
        let gate = Arc::new(Gate::default());
        // Lets the try go however the test ends: a try left parked would
        // keep the runtime from shutting down, so a failure would hang.
        struct LetGo(Arc<Gate>);
        impl Drop for LetGo {
            fn drop(&mut self) {
                self.0.go();
            }
        }
        let let_go = LetGo(Arc::clone(&gate));
        let tries = FolderTries {
            held: Arc::clone(&held) as _,
            limit: Duration::from_millis(50),
            gate: Some(Arc::clone(&gate)),
            window: None,
        };
        let path = folder.display().to_string();
        let result = remove_folder(&path, &tries).await;
        drop(tries);
        let Err(VcsError::Failed { message, .. }) = &result else {
            panic!("{result:?}");
        };
        assert!(message.contains("did not finish"), "{message}");
        // Answered, and the stuck try still holds it.
        gate.reached().await;
        assert_eq!(Arc::strong_count(&held), 2);
        drop(let_go);
        // Its thread returns, and only then lets go, having deleted nothing:
        // Core had called it off.
        let waited = std::time::Instant::now();
        while Arc::strong_count(&held) > 1 && waited.elapsed() < Duration::from_secs(10) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(Arc::strong_count(&held), 1);
        assert!(folder.join("a.txt").exists());
    }

    // No requirement names the cap: it keeps a folder that hangs from
    // taking all of Core's blocking threads, as `attachment.inspect`'s does.
    #[tokio::test]
    async fn blocking_reads_past_the_cap_are_refused_not_queued() {
        let _one_at_a_time = CAP_TESTS.lock().await;
        // Eight reads that have not answered hold every place.
        let held = FILE_SYSTEM.try_acquire_many(8).unwrap();
        let result = blocking(deadline(), "reading", |_| Ok(())).await;
        assert!(matches!(result, Err(VcsError::Busy)), "{result:?}");
        // workspace.changes fails only with `vcs_error`.
        let answered = crate::api::ApiError::from(VcsError::Busy);
        assert_eq!(
            answered.0.data.as_ref().map(|d| d.kind),
            Some(ersa_protocol::core_api::ErrorKind::VcsError),
            "{answered:?}"
        );
        drop(held);
        assert!(blocking(deadline(), "reading", |_| Ok(())).await.is_ok());
    }

    #[tokio::test]
    async fn a_job_keeps_its_place_until_its_thread_returns() {
        let _one_at_a_time = CAP_TESTS.lock().await;
        let free = || FILE_SYSTEM.available_permits();
        assert_eq!(free(), 8);
        // A read the file system does not answer until told to.
        let (answer, hung) = std::sync::mpsc::channel::<()>();
        let short = Deadline::after(Duration::from_millis(50));
        let result = blocking(short, "reading", move |_| {
            let _ = hung.recv();
            Ok(())
        })
        .await;
        assert!(matches!(result, Err(VcsError::Failed { .. })), "{result:?}");
        // Answered at its deadline, the job still holds its place.
        assert_eq!(free(), 7);
        answer.send(()).unwrap();
        // Its thread returns, and only then gives the place back.
        let waited = std::time::Instant::now();
        while free() < 8 && waited.elapsed() < Duration::from_secs(10) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(free(), 8);
    }

    #[tokio::test]
    async fn git_never_reads_a_repository_named_by_inherited_variables() {
        /// Keeps the spec of the one process it is asked to start.
        struct Recording(std::sync::Mutex<Option<ProcessSpec>>);
        #[ersa_adapter::async_trait]
        impl Transport for Recording {
            async fn spawn(
                &self,
                spec: ProcessSpec,
            ) -> std::io::Result<Box<dyn ersa_adapter::Process>> {
                *self.0.lock().unwrap() = Some(spec.clone());
                ersa_adapter::fake::FakeTransport::new(Vec::<String>::new())
                    .spawn(spec)
                    .await
            }
        }
        let transport = Recording(Default::default());
        git(&transport, "repo", &["status"]).await.unwrap();
        let spec = transport.0.lock().unwrap().take().unwrap();
        for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
            assert!(spec.env_remove.iter().any(|n| n == name), "{name}");
        }
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
