//! Core's transport for the machine it runs on.
//!
//! `ersa-adapter` has a plain local transport that kills only the direct
//! child. Core needs more: a backend's whole process tree killed as a unit,
//! no console windows, `.cmd` shims found on Windows, and a cap on how long
//! one line may grow. Those are platform-specific, so they live behind
//! [`platform`], and this transport is what Core hands adapters and the VCS
//! layer instead.

use std::io;
use std::process::Stdio;
use std::sync::{Arc, Weak};

use ersa_adapter::{LineReader, LineWriter, Process, ProcessExit, ProcessSpec, Transport};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

use crate::platform::{self, ProcessTree};

/// The longest line read from a process's stdout or stderr, in bytes: far
/// above any line a backend writes (a tool result with an image in it is a
/// few megabytes), low enough that a process writing without newlines cannot
/// exhaust Core's memory.
pub const MAX_PROCESS_LINE_BYTES: usize = 64 * 1024 * 1024;

/// Spawns processes on the local machine, each in its own [`ProcessTree`].
#[derive(Debug, Clone, Copy)]
pub struct LocalTreeTransport {
    max_line_bytes: usize,
    /// Whether what a process leaves running is killed when Core lets go of
    /// it, not only when it is killed.
    kill_on_drop: bool,
}

impl LocalTreeTransport {
    /// For backends: a session that has ended leaves nothing behind, and
    /// neither does a Core that dies.
    pub fn new() -> Self {
        LocalTreeTransport {
            max_line_bytes: MAX_PROCESS_LINE_BYTES,
            kill_on_drop: true,
        }
    }

    /// For short commands (git, version checks): the tree is killed only when
    /// the command is, on a timeout. A command that finishes may leave a
    /// helper it means to keep, such as git's file-system monitor or a
    /// credential cache.
    pub fn for_commands() -> Self {
        LocalTreeTransport {
            kill_on_drop: false,
            ..LocalTreeTransport::new()
        }
    }

    /// A transport with a different line cap, for tests.
    pub fn with_max_line_bytes(max_line_bytes: usize) -> Self {
        LocalTreeTransport {
            max_line_bytes,
            ..LocalTreeTransport::new()
        }
    }
}

impl Default for LocalTreeTransport {
    fn default() -> Self {
        LocalTreeTransport::new()
    }
}

#[ersa_adapter::async_trait]
impl Transport for LocalTreeTransport {
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        let program =
            platform::resolve_program(&spec.program, spec.env.get("PATH").map(String::as_str));
        let mut command = Command::new(program);
        // Removals first, so `env` can still set a removed name.
        for name in &spec.env_remove {
            command.env_remove(name);
        }
        command
            .args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // A belt to the tree's braces: if the process handle is dropped
            // without a wait, the direct child is killed too.
            .kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        ProcessTree::prepare(&mut command);

        let mut child = command.spawn()?;
        let tree = match ProcessTree::adopt(&child, self.kill_on_drop) {
            Ok(tree) => Arc::new(tree),
            Err(e) => {
                // A child Core cannot kill as a tree is not one to keep.
                child.kill().await.ok();
                return Err(e);
            }
        };
        // The readers hold the tree weakly: only to kill it when a line runs
        // past the cap. Holding it strongly would keep a finished tree alive
        // for as long as someone held the reader, and a reader can wait
        // forever on a pipe that a leftover grandchild keeps open.
        let reader = |stream: Box<dyn AsyncRead + Send + Unpin>| -> Box<dyn LineReader> {
            Box::new(CappedReader::new(
                stream,
                self.max_line_bytes,
                Arc::downgrade(&tree),
            ))
        };
        let stdin = child
            .stdin
            .take()
            .map(|s| Box::new(LocalWriter(s)) as Box<dyn LineWriter>);
        let stdout = child.stdout.take().map(|s| reader(Box::new(s)));
        let stderr = child.stderr.take().map(|s| reader(Box::new(s)));
        Ok(Box::new(TreeProcess {
            child,
            tree,
            stdin,
            stdout,
            stderr,
        }))
    }
}

/// A process and its tree. Dropping it kills whatever is left of the tree.
struct TreeProcess {
    child: Child,
    tree: Arc<ProcessTree>,
    stdin: Option<Box<dyn LineWriter>>,
    stdout: Option<Box<dyn LineReader>>,
    stderr: Option<Box<dyn LineReader>>,
}

#[ersa_adapter::async_trait]
impl Process for TreeProcess {
    fn take_stdin(&mut self) -> Option<Box<dyn LineWriter>> {
        self.stdin.take()
    }

    fn take_stdout(&mut self) -> Option<Box<dyn LineReader>> {
        self.stdout.take()
    }

    fn take_stderr(&mut self) -> Option<Box<dyn LineReader>> {
        self.stderr.take()
    }

    async fn wait(&mut self) -> io::Result<ProcessExit> {
        // Cancellation-safe, and remembers the status once the child exited.
        let status = self.child.wait().await?;
        // Before anyone hears of the exit: what the process left running
        // goes now (for a backend), so a stop that waits for the exit waits
        // for the whole tree. Repeated calls repeat a harmless no-op.
        self.tree.leader_exited();
        Ok(ProcessExit {
            code: status.code(),
        })
    }

    async fn kill(&mut self) -> io::Result<()> {
        // The whole tree, not only the child: this is what a stop that ran
        // out of patience means. Then reap the child, as `Process::kill`
        // promises it has exited when this returns.
        let killed = self.tree.kill();
        self.child.kill().await?;
        // The leader is reaped now and the tree was just signalled, so its
        // group is retired without signalling it again.
        self.tree.retire();
        killed
    }
}

struct LocalWriter(ChildStdin);

#[ersa_adapter::async_trait]
impl LineWriter for LocalWriter {
    async fn write_line(&mut self, line: &str) -> io::Result<()> {
        if line.contains('\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a line written to a process must not contain a newline",
            ));
        }
        // One buffer and one write, so the line and its terminator reach the
        // pipe together.
        let mut bytes = Vec::with_capacity(line.len() + 1);
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        self.0.write_all(&bytes).await?;
        self.0.flush().await
    }
}

/// Reads lines of at most `max` bytes from a pipe.
///
/// A process that writes past the cap without a newline is killed, tree and
/// all, and the read fails: nothing it says after that can be trusted to
/// line up, and a reader that skipped ahead would put a line in the raw log
/// that the process never wrote.
struct CappedReader {
    reader: BufReader<Box<dyn AsyncRead + Send + Unpin>>,
    max: usize,
    tree: Weak<ProcessTree>,
    /// The line read so far. A field, so a cancelled call loses nothing.
    line: Vec<u8>,
    /// Set once the cap was hit; every later read fails the same way.
    overflowed: bool,
}

impl CappedReader {
    fn new(stream: Box<dyn AsyncRead + Send + Unpin>, max: usize, tree: Weak<ProcessTree>) -> Self {
        CappedReader {
            reader: BufReader::new(stream),
            max,
            tree,
            line: Vec::new(),
            overflowed: false,
        }
    }

    fn too_long(&self) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the process wrote a line longer than {} bytes", self.max),
        )
    }
}

#[ersa_adapter::async_trait]
impl LineReader for CappedReader {
    async fn next_line(&mut self) -> io::Result<Option<String>> {
        if self.overflowed {
            return Err(self.too_long());
        }
        loop {
            // `fill_buf` then `consume` rather than `read_until`, so the cap
            // is checked before the bytes are kept. Both are cancellation
            // safe: bytes move into `self.line` only between awaits.
            let available = self.reader.fill_buf().await?;
            if available.is_empty() {
                if self.line.is_empty() {
                    return Ok(None);
                }
                // A last line without a newline still counts.
                return Ok(Some(finish(std::mem::take(&mut self.line))));
            }
            let (chunk, found_newline) = match available.iter().position(|&b| b == b'\n') {
                Some(at) => (&available[..at], true),
                None => (available, false),
            };
            if self.line.len() + chunk.len() > self.max {
                self.overflowed = true;
                self.line = Vec::new();
                tracing::error!(
                    max = self.max,
                    "a process wrote an over-long line; killing it"
                );
                if let Some(tree) = self.tree.upgrade() {
                    tree.kill().ok();
                }
                return Err(self.too_long());
            }
            self.line.extend_from_slice(chunk);
            let used = chunk.len() + usize::from(found_newline);
            self.reader.consume(used);
            if found_newline {
                return Ok(Some(finish(std::mem::take(&mut self.line))));
            }
        }
    }
}

/// A line's bytes as text, without a trailing `\r`. Bytes that are not UTF-8
/// become U+FFFD instead of an error, so one bad line from a backend cannot
/// end the loop reading it.
fn finish(mut bytes: Vec<u8>) -> String {
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
