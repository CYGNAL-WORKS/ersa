//! Unix: a Unix domain socket in a directory only the current user can enter,
//! and clients that talk only to a socket the current user serves.
//!
//! Compiles but is untested: CI runs on Windows only (ADR 0006). Kept so that
//! nothing outside the platform module assumes Windows.

// `geteuid` is a libc call, and every libc call is `unsafe` in Rust.
#![allow(unsafe_code)]

use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
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

/// Accepts connections on one socket path.
pub struct Listener {
    inner: UnixListener,
}

impl Listener {
    /// Binds the socket inside a directory only this user can enter.
    ///
    /// Fails with `AddrInUse` when a process of this user is accepting on the
    /// socket, and `PermissionDenied` when the directory or an existing
    /// socket belongs to someone else. A leftover socket of this user's that
    /// nobody accepts on is removed and replaced.
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
            std::fs::remove_file(path)?;
        }
        let inner = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener { inner })
    }

    /// Waits for the next client.
    pub async fn accept(&mut self) -> io::Result<BoxStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(Box::new(stream))
    }
}

/// Makes sure `dir` exists, belongs to this user, and admits nobody else.
fn private_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let meta = std::fs::metadata(dir)?;
    if meta.uid() != euid() || meta.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} must belong to this user and admit nobody else",
                dir.display()
            ),
        ));
    }
    Ok(())
}

/// Opens a client connection to `endpoint` and checks that the process
/// serving it runs as the current user, before anything is sent.
pub async fn connect(endpoint: &str) -> io::Result<BoxStream> {
    let stream = UnixStream::connect(endpoint).await?;
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
