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

/// Runs `spec` to completion through `transport`, with no input, and
/// collects its output. Gives up and kills the process after `timeout`.
pub async fn run(
    transport: &dyn Transport,
    spec: ProcessSpec,
    timeout: Duration,
) -> io::Result<Output> {
    let mut process = transport.spawn(spec).await?;
    // Dropping stdin closes it, so a command that would read input sees
    // end-of-file instead of waiting forever.
    drop(process.take_stdin());
    let stdout = process.take_stdout();
    let stderr = process.take_stderr();

    // Read both streams at once: a process that fills one pipe while we wait
    // on the other would block forever.
    let collect = async { tokio::try_join!(read_all(stdout), read_all(stderr)) };
    match tokio::time::timeout(timeout, collect).await {
        Ok(Ok((stdout, stderr))) => {
            let exit = process.wait().await?;
            Ok(Output {
                exit,
                stdout,
                stderr,
            })
        }
        Ok(Err(e)) => {
            process.kill().await.ok();
            Err(e)
        }
        Err(_) => {
            process.kill().await.ok();
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("the command did not finish within {timeout:?}"),
            ))
        }
    }
}

async fn read_all(reader: Option<Box<dyn LineReader>>) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    if let Some(mut reader) = reader {
        while let Some(line) = reader.next_line().await? {
            lines.push(line);
        }
    }
    Ok(lines)
}
