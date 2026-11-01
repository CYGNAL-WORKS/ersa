//! The transport for the machine Core runs on, built on `tokio::process`.

use std::io;
use std::process::Stdio;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, Command};

use crate::transport::{LineReader, LineWriter, Process, ProcessExit, ProcessSpec, Transport};

/// Spawns processes on the local machine.
///
/// This kills only the direct child. Killing a backend's whole process tree
/// (job objects on Windows) and hiding console windows are Core's, behind a
/// platform module, so this file stays free of platform-specific code.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalTransport;

#[async_trait]
impl Transport for LocalTransport {
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // If the `LocalProcess` is dropped without being waited on, kill the
            // child rather than leave it running unowned.
            .kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }

        let mut child = command.spawn()?;
        // With all three set to `Stdio::piped()`, the handles are always there.
        let stdin = child.stdin.take().map(|s| Box::new(LocalWriter(s)) as _);
        let stdout = child
            .stdout
            .take()
            .map(|s| Box::new(LocalReader::new(s)) as _);
        let stderr = child
            .stderr
            .take()
            .map(|s| Box::new(LocalReader::new(s)) as _);
        Ok(Box::new(LocalProcess {
            child,
            stdin,
            stdout,
            stderr,
        }))
    }
}

struct LocalProcess {
    child: Child,
    stdin: Option<Box<dyn LineWriter>>,
    stdout: Option<Box<dyn LineReader>>,
    stderr: Option<Box<dyn LineReader>>,
}

#[async_trait]
impl Process for LocalProcess {
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
        // `Child::wait` is cancellation-safe and remembers the status once the
        // child has exited, which gives `Process::wait` both of its promises.
        let status = self.child.wait().await?;
        Ok(ProcessExit {
            code: status.code(),
        })
    }

    async fn kill(&mut self) -> io::Result<()> {
        // `Child::kill` sends the kill and then waits, and treats a child that
        // already exited as success.
        self.child.kill().await
    }
}

struct LocalWriter(ChildStdin);

#[async_trait]
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

/// Reads lines from any byte stream; used for both stdout and stderr.
///
/// `Box<dyn AsyncRead + Send + Unpin>` lets one type wrap either pipe. `Unpin`
/// (safe to move after being polled) is what `Lines::next_line` asks of the
/// reader so it can poll it through a plain `&mut`.
struct LocalReader {
    lines: Lines<BufReader<Box<dyn AsyncRead + Send + Unpin>>>,
}

impl LocalReader {
    fn new(stream: impl AsyncRead + Send + Unpin + 'static) -> Self {
        let stream: Box<dyn AsyncRead + Send + Unpin> = Box::new(stream);
        LocalReader {
            lines: BufReader::new(stream).lines(),
        }
    }
}

#[async_trait]
impl LineReader for LocalReader {
    async fn next_line(&mut self) -> io::Result<Option<String>> {
        // `Lines::next_line` keeps a partly read line in its own buffer when
        // its future is dropped, which is what makes this cancellation-safe.
        // It strips `\n` and `\r\n`.
        self.lines.next_line().await
    }
}
