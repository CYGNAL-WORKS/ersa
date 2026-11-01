//! Transport layer: starts processes and carries their stdio on a host, by
//! local spawn or over SSH. Adapters and the workspace and VCS layer run through
//! it without knowing which transport they got.
//!
//! The traits and the local transport live in `ersa-adapter`, which adapters
//! depend on too. This module adds what Core needs on top: running a short
//! command to completion and collecting its output, for VCS commands and
//! backend detection.

use std::io;
use std::time::Duration;

use ersa_adapter::{LineReader, ProcessExit, ProcessSpec, Transport};

/// What a finished command printed, and how it exited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub exit: ProcessExit,
    pub stdout: Vec<String>,
    pub stderr: Vec<String>,
}

impl Output {
    /// Stderr as one string, for error messages.
    pub fn stderr_text(&self) -> String {
        self.stderr.join("\n").trim().to_string()
    }
}

/// Why [`run`] produced no [`Output`].
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// The command could not be started (`NotFound` when it does not exist).
    #[error("could not start the command: {0}")]
    Spawn(io::Error),
    /// It started, but did not finish in time or its output could not be
    /// read. It has been killed; whatever it was doing may be half done.
    #[error("the command did not complete: {0}")]
    Incomplete(io::Error),
}

/// Stands in for a line of output that was not UTF-8. The transport's line
/// readers return only text, so such a line is reported and skipped rather
/// than passed on; one stray byte must not fail an otherwise healthy command.
pub const UNREADABLE_LINE: &str = "\u{FFFD}";

/// More unreadable lines than this in one stream means the stream is not
/// text at all, and reading stops rather than looping.
const MAX_UNREADABLE_LINES: usize = 1000;

/// Runs `spec` to completion through `transport`, with no input, and
/// collects its output. Gives up and kills the process after `timeout`.
pub async fn run(
    transport: &dyn Transport,
    spec: ProcessSpec,
    timeout: Duration,
) -> Result<Output, RunError> {
    let mut process = transport.spawn(spec).await.map_err(RunError::Spawn)?;
    // Dropping stdin closes it, so a command that would read input sees
    // end-of-file instead of waiting forever.
    drop(process.take_stdin());
    let stdout = process.take_stdout();
    let stderr = process.take_stderr();

    // Read both streams at once: a process that fills one pipe while we wait
    // on the other would block forever.
    let collect = async { tokio::try_join!(read_all(stdout), read_all(stderr)) };
    let failure = match tokio::time::timeout(timeout, collect).await {
        Ok(Ok((stdout, stderr))) => {
            let exit = process.wait().await.map_err(RunError::Incomplete)?;
            return Ok(Output {
                exit,
                stdout,
                stderr,
            });
        }
        Ok(Err(e)) => e,
        Err(_) => io::Error::new(
            io::ErrorKind::TimedOut,
            format!("the command did not finish within {timeout:?}"),
        ),
    };
    process.kill().await.ok();
    Err(RunError::Incomplete(failure))
}

/// Every line of `reader`. A line that is not UTF-8 becomes
/// [`UNREADABLE_LINE`]: the local transport reports it as `InvalidData` and
/// moves past it, so reading can go on.
async fn read_all(reader: Option<Box<dyn LineReader>>) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let Some(mut reader) = reader else {
        return Ok(lines);
    };
    let mut unreadable = 0;
    loop {
        match reader.next_line().await {
            Ok(Some(line)) => lines.push(line),
            Ok(None) => return Ok(lines),
            Err(e)
                if e.kind() == io::ErrorKind::InvalidData && unreadable < MAX_UNREADABLE_LINES =>
            {
                unreadable += 1;
                lines.push(UNREADABLE_LINE.to_string());
            }
            Err(e) => return Err(e),
        }
    }
}
