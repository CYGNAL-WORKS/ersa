//! Unix: a Unix domain socket that only the current user can open.
//!
//! Compiles but is untested: CI runs on Windows only (ADR 0006). Kept so that
//! nothing outside the platform module assumes Windows.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use tokio::net::{UnixListener, UnixStream};

use super::BoxStream;

/// `$XDG_RUNTIME_DIR/ersa-core.sock`, which is already private to the user;
/// otherwise a per-user name in the temporary directory.
pub fn default_endpoint() -> String {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir)
            .join("ersa-core.sock")
            .to_string_lossy()
            .into_owned(),
        None => {
            let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
            std::env::temp_dir()
                .join(format!("ersa-core-{user}.sock"))
                .to_string_lossy()
                .into_owned()
        }
    }
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

/// Accepts connections on one socket path.
pub struct Listener {
    inner: UnixListener,
}

impl Listener {
    /// Binds the socket. Fails with `io::ErrorKind::AddrInUse` when another
    /// process is accepting on it; a leftover socket file from a Core that
    /// exited without cleaning up is removed and replaced.
    pub fn bind(endpoint: &str) -> io::Result<Listener> {
        if std::path::Path::new(endpoint).exists() {
            if std::os::unix::net::UnixStream::connect(endpoint).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("another process already owns {endpoint}"),
                ));
            }
            std::fs::remove_file(endpoint)?;
        }
        let inner = UnixListener::bind(endpoint)?;
        std::fs::set_permissions(endpoint, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener { inner })
    }

    /// Waits for the next client.
    pub async fn accept(&mut self) -> io::Result<BoxStream> {
        let (stream, _) = self.inner.accept().await?;
        Ok(Box::new(stream))
    }
}

/// Opens a client connection to `endpoint`.
pub async fn connect(endpoint: &str) -> io::Result<BoxStream> {
    Ok(Box::new(UnixStream::connect(endpoint).await?))
}
