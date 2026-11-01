//! Starting processes on a host and carrying their stdio, line by line.
//!
//! Adapters and the workspace layer use these traits and never a concrete
//! transport, so the same adapter runs over local spawn and SSH (ARCH-CORE-R04).
//! Errors are plain `std::io::Error`: every failure here is an I/O failure, and
//! `io::ErrorKind` already says which kind.

use std::collections::BTreeMap;
use std::io;

use async_trait::async_trait;

/// Starts processes on one host.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Starts `spec.program` with stdin, stdout, and stderr piped. Backends are
    /// never run in a pseudo-terminal.
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>>;
}

/// What to start.
///
/// Paths are `String`s, not `PathBuf`s, because they are paths on the host the
/// process runs on, which may not be this machine: a Windows Core starting a
/// process on a Linux host over SSH must not reinterpret `/home/me/repo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessSpec {
    /// The executable, as a path or a name the host resolves.
    pub program: String,
    pub args: Vec<String>,
    /// The working directory; the transport's default when `None`.
    pub cwd: Option<String>,
    /// Variables added to (or overriding) the environment the process inherits.
    /// A `BTreeMap` so the order is stable in logs and test assertions.
    pub env: BTreeMap<String, String>,
    /// Variables removed from the inherited environment. Removal happens
    /// before `env` is applied, so a name in both ends up set.
    pub env_remove: Vec<String>,
}

/// A started process.
///
/// Its three streams are taken out once, like `tokio::process::Child`, so that
/// one task can read stdout while another writes stdin. What stays behind is
/// the handle for waiting on and killing the process.
///
/// Whoever takes stderr must keep reading it: a process whose stderr pipe fills
/// up blocks. Dropping the reader is fine too; then its output is discarded.
#[async_trait]
pub trait Process: Send {
    /// The process's stdin. `None` after the first call.
    fn take_stdin(&mut self) -> Option<Box<dyn LineWriter>>;
    /// The process's stdout. `None` after the first call.
    fn take_stdout(&mut self) -> Option<Box<dyn LineReader>>;
    /// The process's stderr. `None` after the first call.
    fn take_stderr(&mut self) -> Option<Box<dyn LineReader>>;

    /// Waits for the process to exit. Calling it again after the process has
    /// exited returns the same result. Cancellation-safe: dropping the future
    /// (for example in `tokio::select!`) leaves the process untouched.
    async fn wait(&mut self) -> io::Result<ProcessExit>;

    /// Kills the process and waits for it to exit. Killing a process that has
    /// already exited is not an error. Only this process is killed; killing its
    /// whole process tree is Core's job, behind a platform module.
    async fn kill(&mut self) -> io::Result<()>;
}

/// Writes whole lines to a process's stdin.
///
/// Dropping the writer closes stdin, which is how a process is told there is no
/// more input.
#[async_trait]
pub trait LineWriter: Send {
    /// Writes `line` followed by `\n` and flushes, so the process sees the line
    /// as soon as this returns. `line` must not contain `\n`; a line that does
    /// fails with `io::ErrorKind::InvalidInput`, since it would arrive as two.
    async fn write_line(&mut self, line: &str) -> io::Result<()>;
}

/// Reads whole lines from a process's stdout or stderr.
#[async_trait]
pub trait LineReader: Send {
    /// The next line without its `\n` or `\r\n`, or `None` once the stream has
    /// ended.
    ///
    /// Cancellation-safe: if the future is dropped before it completes, no data
    /// is lost, and the next call returns the line the dropped one would have.
    /// Read loops depend on this to `select!` between a line and a stop signal.
    async fn next_line(&mut self) -> io::Result<Option<String>>;
}

/// How a process ended.
///
/// Ersa's own type rather than `std::process::ExitStatus`, which cannot be
/// built by hand without platform-specific code and which a remote transport
/// never has: over SSH, the exit code is all that comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessExit {
    /// The exit code, or `None` if the process ended without one (killed by a
    /// signal on Unix, or the transport could not tell).
    pub code: Option<i32>,
}

impl ProcessExit {
    /// Whether the process exited with code 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}
