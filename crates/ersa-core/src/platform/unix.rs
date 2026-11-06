//! Unix: a Unix domain socket in a directory only the current user can enter,
//! and clients that talk only to a socket the current user serves.
//!
//! Tested in CI on Linux. Windows remains the first platform (ADR 0006);
//! this is what keeps Core's socket suite running everywhere else.

// `geteuid` is a libc call, and every libc call is `unsafe` in Rust.
#![allow(unsafe_code)]

use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

use super::BoxStream;

/// `$XDG_RUNTIME_DIR/ersa-core.sock` (that directory is already private to
/// the user); otherwise `ersa-core-<uid>/ersa-core.sock` in the temporary
/// directory, a directory `bind` creates private.
pub fn default_endpoint() -> io::Result<String> {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::temp_dir().join(format!("ersa-core-{}", euid())),
    };
    Ok(dir.join("ersa-core.sock").to_string_lossy().into_owned())
}

/// `ersa-core-<label>.sock` beside the default socket, in the same private
/// directory: an endpoint of this user's that is not the default, for tests
/// and a second Core. See [`super::check_label`] for what `label` may hold.
pub fn labelled_endpoint(label: &str) -> io::Result<String> {
    super::check_label(label)?;
    let default = PathBuf::from(default_endpoint()?);
    let path = default.with_file_name(format!("ersa-core-{label}.sock"));
    // A socket path must fit `sun_path` with its terminating NUL; a long
    // temporary directory (macOS) plus a long label may not. Say so here
    // rather than leave bind to fail with a vaguer error.
    let len = path.as_os_str().len();
    if len >= SUN_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is {len} bytes; a socket path must be under {SUN_PATH}: use a shorter label",
                path.display()
            ),
        ));
    }
    Ok(path.to_string_lossy().into_owned())
}

/// The size of `sockaddr_un.sun_path`: 108 bytes on Linux, 104 on macOS and
/// the BSDs.
const SUN_PATH: usize = if cfg!(target_os = "linux") || cfg!(target_os = "android") {
    108
} else {
    104
};

/// `$XDG_DATA_HOME/ersa`, or `~/.local/share/ersa`.
pub fn default_data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(dir).join("ersa"));
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/ersa"))
}

/// The machine's host name.
pub fn host_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

/// The canonical form of an existing path.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// The key two spellings of one path share. Unix file systems are case
/// sensitive, so only a trailing separator is dropped.
pub fn path_key(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() { "/" } else { trimmed }.to_string()
}

/// Accepts connections on one socket path, and removes the socket file when
/// dropped, so a stopped Core leaves nothing behind for the next to clear.
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Binds the socket inside a directory only this user can enter.
    ///
    /// Fails with `AddrInUse` when a process of this user is accepting on the
    /// socket, with `PermissionDenied` when an existing socket belongs to
    /// someone else, and with another error when the directory is not
    /// private. A leftover socket of this user's that nobody accepts on is
    /// removed and replaced.
    pub async fn bind(endpoint: &str) -> io::Result<Listener> {
        let path = Path::new(endpoint);
        let dir = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "the socket path has no directory",
            )
        })?;
        private_dir(dir)?;

        if let Ok(meta) = std::fs::symlink_metadata(path) {
            if meta.uid() != euid() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("{endpoint} belongs to another user"),
                ));
            }
            if connect(endpoint).await.is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("a process running as this user already serves {endpoint}"),
                ));
            }
            // Only a leftover socket is cleared: an explicit `--endpoint`
            // naming some other file of the user's must not delete it.
            if !meta.file_type().is_socket() {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("{endpoint} exists and is not a socket"),
                ));
            }
            std::fs::remove_file(path)?;
        }
        let inner = UnixListener::bind(path)?;
        // The directory already keeps others out; this keeps the socket
        // itself private too, whatever the umask made it.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener {
            inner,
            path: path.to_path_buf(),
        })
    }

    /// Waits for the next client.
    pub async fn accept(&mut self) -> io::Result<BoxStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(Box::new(stream))
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

/// Makes sure `dir` exists, belongs to this user, and admits nobody else.
/// A directory someone else created first (in a shared `/tmp`, say) is
/// refused rather than used.
fn private_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let meta = std::fs::metadata(dir)?;
    if meta.uid() != euid() || meta.mode() & 0o077 != 0 {
        // Not `PermissionDenied`, which `bind` reserves for a socket held by
        // another user: this is a setup problem, not a squatter.
        return Err(io::Error::other(format!(
            "{} must belong to this user and admit nobody else (mode 0700)",
            dir.display()
        )));
    }
    Ok(())
}

/// Opens a client connection to `endpoint` and checks that the process
/// serving it runs as the current user, before anything is sent.
pub async fn connect(endpoint: &str) -> io::Result<BoxStream> {
    let stream = UnixStream::connect(endpoint).await?;
    // The kernel's record of who is on the other end: `SO_PEERCRED` on
    // Linux, `getpeereid` on macOS and the BSDs.
    let peer = stream.peer_cred()?;
    if peer.uid() != euid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("refusing {endpoint}: it is served by another user's process"),
        ));
    }
    Ok(Box::new(stream))
}

fn euid() -> u32 {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

// -- Child processes ----------------------------------------------------------

/// A started process and everything it starts in turn, killed as one unit.
///
/// On Unix that unit is a process group: the child leads a new group, and
/// what it starts joins the group unless it deliberately leaves (a daemon
/// that calls `setsid` does). Dropping a `ProcessTree` made with
/// `kill_on_drop` kills the group.
pub struct ProcessTree {
    /// The group's id, which is the child's process id.
    group: i32,
    kill_on_drop: bool,
    /// Set once the leader has been reaped and the leftovers dealt with.
    /// After that the group's id may be freed and reused by an unrelated
    /// group, so it is never signalled again.
    spent: std::sync::atomic::AtomicBool,
}

impl ProcessTree {
    /// Sets up `command` before it is spawned: the child leads a new process
    /// group, so killing the group can never reach Core itself.
    pub fn prepare(command: &mut tokio::process::Command) {
        command.process_group(0);
    }

    /// Remembers a just-spawned `child`'s group. With `kill_on_drop`,
    /// dropping the tree kills what is left of it.
    pub fn adopt(child: &tokio::process::Child, kill_on_drop: bool) -> io::Result<ProcessTree> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the process exited before its group was known"))?;
        let group = i32::try_from(pid).map_err(io::Error::other)?;
        Ok(ProcessTree {
            group,
            kill_on_drop,
            spent: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Called as soon as the leader has been reaped: kills the leftovers
    /// when the tree was made with `kill_on_drop`, then retires the group's
    /// id, which is never signalled again. While any member lives the
    /// kernel keeps the id reserved, so the kill reaches only this group.
    /// When none is left the id is free from the moment the leader was
    /// reaped, and another group taking it in the microseconds before this
    /// signal would be hit; nothing portable closes that gap (pidfd groups
    /// are Linux-only). If Core itself dies, nothing is signalled at all:
    /// the leftovers outlive it on Unix.
    pub fn leader_exited(&self) {
        // Once retired, never again: the id may belong to someone else.
        if self.spent.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        if self.kill_on_drop {
            let _ = self.signal();
        }
    }

    /// Retires the group's id without signalling it: for a leader that was
    /// just killed and reaped, whose group was signalled already.
    pub fn retire(&self) {
        self.spent.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Kills every process in the group. A group that has already ended is
    /// not an error.
    pub fn kill(&self) -> io::Result<()> {
        if self.spent.load(std::sync::atomic::Ordering::SeqCst) {
            // The leader is gone and the leftovers were dealt with then.
            return Ok(());
        }
        self.signal()
    }

    fn signal(&self) -> io::Result<()> {
        // SAFETY: `kill` has no memory-safety preconditions. A negative pid
        // addresses the whole process group.
        if unsafe { libc::kill(-self.group, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // ESRCH: nothing is left in the group.
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        // A tree dropped before its leader was reaped (a failed start, say):
        // the leader is a zombie at worst, which keeps the id reserved, so
        // the signal reaches only this group. `kill` does nothing once the
        // id is retired.
        if self.kill_on_drop {
            let _ = self.kill();
        }
    }
}

/// Unix names no pipe or device by its name: its metadata tells one,
/// without opening it.
pub fn names_device(_path: &Path) -> bool {
    false
}

/// The file to start for `program`: Unix needs no help finding it.
pub fn resolve_program(program: &str, _path: Option<&str>) -> String {
    program.to_string()
}
