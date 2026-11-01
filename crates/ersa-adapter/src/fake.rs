//! A scripted stand-in for a real transport, for tests in any crate. Compiled
//! only with the `test-support` feature.
//!
//! ```
//! # use ersa_adapter::{fake::FakeTransport, ProcessSpec, Transport};
//! # tokio_test_block_on(async {
//! let transport = FakeTransport::new(["{\"type\":\"hello\"}"]).with_exit_code(0);
//! let mut process = transport.spawn(ProcessSpec::default()).await.unwrap();
//!
//! let mut stdin = process.take_stdin().unwrap();
//! stdin.write_line("{\"type\":\"ping\"}").await.unwrap();
//!
//! let mut stdout = process.take_stdout().unwrap();
//! assert_eq!(stdout.next_line().await.unwrap().as_deref(), Some("{\"type\":\"hello\"}"));
//! assert_eq!(stdout.next_line().await.unwrap(), None);
//!
//! assert_eq!(transport.stdin_lines(), ["{\"type\":\"ping\"}"]);
//! # });
//! # fn tokio_test_block_on(f: impl std::future::Future<Output = ()>) {
//! #     tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
//! # }
//! ```

use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::watch;

use crate::transport::{LineReader, LineWriter, Process, ProcessExit, ProcessSpec, Transport};

/// A transport whose processes replay scripted stdout and stderr lines and
/// record what is written to their stdin.
///
/// Clones share one record, so a test can keep a clone while handing another
/// to the code under test, then inspect what it spawned and wrote.
#[derive(Debug, Clone)]
pub struct FakeTransport {
    script: Script,
    record: Arc<Mutex<Record>>,
}

#[derive(Debug, Clone)]
struct Script {
    stdout: Vec<String>,
    stderr: Vec<String>,
    exit_code: Option<i32>,
    hold_open: bool,
}

#[derive(Debug, Default)]
struct Record {
    spawned: Vec<ProcessSpec>,
    stdin: Vec<String>,
}

impl FakeTransport {
    /// Every process this transport spawns writes `stdout` lines, in order,
    /// then ends its stdout and exits with code 0.
    pub fn new<I, S>(stdout: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        FakeTransport {
            script: Script {
                stdout: stdout.into_iter().map(Into::into).collect(),
                stderr: Vec::new(),
                exit_code: Some(0),
                hold_open: false,
            },
            record: Arc::default(),
        }
    }

    /// Also writes these lines to stderr.
    pub fn with_stderr<I, S>(mut self, stderr: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.script.stderr = stderr.into_iter().map(Into::into).collect();
        self
    }

    /// Exits with this code instead of 0; `None` for an exit without a code.
    pub fn with_exit_code(mut self, code: impl Into<Option<i32>>) -> Self {
        self.script.exit_code = code.into();
        self
    }

    /// After the scripted stdout lines, keeps stdout open and the process
    /// running until it is killed or its stdin is closed, like a real backend
    /// waiting for input. Without this, the process ends as soon as its stdout
    /// has been read.
    pub fn hold_open(mut self) -> Self {
        self.script.hold_open = true;
        self
    }

    /// Every spec passed to `spawn`, in order.
    pub fn spawned(&self) -> Vec<ProcessSpec> {
        self.lock().spawned.clone()
    }

    /// Every line written to any spawned process's stdin, in order.
    pub fn stdin_lines(&self) -> Vec<String> {
        self.lock().stdin.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Record> {
        // A poisoned lock means another test thread panicked while holding it;
        // the record itself is still usable.
        self.record.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[async_trait]
impl Transport for FakeTransport {
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        self.lock().spawned.push(spec);

        // `ended` flips to true when the process is killed or its stdin closes.
        // A held-open stdout and `wait` watch it to know when to finish.
        let (ended_tx, ended_rx) = watch::channel(!self.script.hold_open);
        let ended = Arc::new(ended_tx);
        let killed = Arc::new(AtomicBool::new(false));
        let stdin = FakeWriter {
            record: self.record.clone(),
            ended: ended.clone(),
            killed: killed.clone(),
        };
        Ok(Box::new(FakeProcess {
            exit: ProcessExit {
                code: self.script.exit_code,
            },
            ended,
            stdin: Some(Box::new(stdin)),
            stdout: Some(Box::new(FakeReader {
                lines: self.script.stdout.clone().into(),
                ended: ended_rx.clone(),
            })),
            stderr: Some(Box::new(FakeReader {
                lines: self.script.stderr.clone().into(),
                // Stderr never holds open; it ends after its scripted lines.
                ended: watch::channel(true).1,
            })),
            ended_rx,
            killed,
        }))
    }
}

struct FakeProcess {
    exit: ProcessExit,
    ended: Arc<watch::Sender<bool>>,
    ended_rx: watch::Receiver<bool>,
    /// Shared with the stdin writer, which fails once the process is killed.
    killed: Arc<AtomicBool>,
    stdin: Option<Box<dyn LineWriter>>,
    stdout: Option<Box<dyn LineReader>>,
    stderr: Option<Box<dyn LineReader>>,
}

#[async_trait]
impl Process for FakeProcess {
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
        wait_until_ended(&mut self.ended_rx).await;
        // A killed process has no exit code, as on Unix after a signal.
        Ok(if self.killed.load(Ordering::SeqCst) {
            ProcessExit { code: None }
        } else {
            self.exit
        })
    }

    async fn kill(&mut self) -> io::Result<()> {
        // A process that is not held open has already exited; killing it does
        // nothing, as `Process::kill` promises.
        if !*self.ended_rx.borrow() {
            self.killed.store(true, Ordering::SeqCst);
            self.ended.send_replace(true);
        }
        Ok(())
    }
}

/// Waits until `ended` is true. Cancellation-safe, as `watch::Receiver::wait_for` is.
async fn wait_until_ended(ended: &mut watch::Receiver<bool>) {
    // `wait_for` fails only if every sender is gone, which also means nothing
    // can ever end the process; treat that as ended.
    let _ = ended.wait_for(|ended| *ended).await;
}

struct FakeWriter {
    record: Arc<Mutex<Record>>,
    ended: Arc<watch::Sender<bool>>,
    killed: Arc<AtomicBool>,
}

#[async_trait]
impl LineWriter for FakeWriter {
    async fn write_line(&mut self, line: &str) -> io::Result<()> {
        if line.contains('\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a line written to a process must not contain a newline",
            ));
        }
        // Writing to a killed process fails as writing to a closed pipe does.
        if self.killed.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let mut record = self.record.lock().unwrap_or_else(|e| e.into_inner());
        record.stdin.push(line.to_string());
        Ok(())
    }
}

/// Closing stdin ends a held-open process, as end of input does a real backend.
impl Drop for FakeWriter {
    fn drop(&mut self) {
        self.ended.send_replace(true);
    }
}

struct FakeReader {
    lines: VecDeque<String>,
    ended: watch::Receiver<bool>,
}

#[async_trait]
impl LineReader for FakeReader {
    async fn next_line(&mut self) -> io::Result<Option<String>> {
        // Taking a line happens in one step with no await before it, so a
        // dropped call can never lose one.
        if let Some(line) = self.lines.pop_front() {
            return Ok(Some(line));
        }
        wait_until_ended(&mut self.ended).await;
        Ok(None)
    }
}
