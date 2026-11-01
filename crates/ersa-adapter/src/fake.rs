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
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::watch;

use crate::transport::{LineReader, LineWriter, Process, ProcessExit, ProcessSpec, Transport};

/// A transport whose processes replay scripted stdout and stderr lines and
/// record what is written to their stdin.
///
/// The stdout script can also pause until the code under test writes a stdin
/// line ([`FakeTransport::wait_for_stdin`]), which turns it into a stub backend
/// that answers each request in turn.
///
/// Clones share one record, so a test can keep a clone while handing another
/// to the code under test, then inspect what it spawned and wrote.
#[derive(Debug, Clone)]
pub struct FakeTransport {
    script: Script,
    record: Arc<Mutex<Record>>,
    /// How many stdin lines have been written so far, so a paused stdout can
    /// wait for the next one without polling.
    stdin_count: Arc<watch::Sender<usize>>,
}

#[derive(Debug, Clone)]
struct Script {
    stdout: Vec<Step>,
    stderr: Vec<String>,
    exit_code: Option<i32>,
    hold_open: bool,
}

/// One step of a scripted stdout.
#[derive(Debug, Clone)]
enum Step {
    Line(String),
    /// Wait for the next stdin line no earlier step has consumed, and check it.
    AwaitStdin(StdinCheck),
}

/// The check a paused stdout applies to the stdin line it waited for. `Err`
/// says what was wrong with the line.
#[derive(Clone)]
struct StdinCheck(Arc<CheckFn>);

/// Shared between clones of the script, and callable from the reader's task.
type CheckFn = dyn Fn(&str) -> Result<(), String> + Send + Sync;

// Closures have no `Debug`, so `derive(Debug)` cannot look inside this one.
impl fmt::Debug for StdinCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StdinCheck(..)")
    }
}

#[derive(Debug, Default)]
struct Record {
    spawned: Vec<ProcessSpec>,
    stdin: Vec<String>,
    failures: Vec<String>,
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
                stdout: stdout.into_iter().map(|l| Step::Line(l.into())).collect(),
                stderr: Vec::new(),
                exit_code: Some(0),
                hold_open: false,
            },
            record: Arc::default(),
            stdin_count: Arc::new(watch::channel(0).0),
        }
    }

    /// After the stdout lines so far, waits for the next stdin line and passes
    /// it to `check`, then carries on with the rest of the script. Each wait
    /// consumes one stdin line, in the order they were written.
    ///
    /// When `check` returns `Err`, the message is kept for
    /// [`FakeTransport::stub_failures`] and the script stops there, as a real
    /// backend that received a line it did not expect would stop answering.
    pub fn wait_for_stdin<F>(mut self, check: F) -> Self
    where
        F: Fn(&str) -> Result<(), String> + Send + Sync + 'static,
    {
        self.script
            .stdout
            .push(Step::AwaitStdin(StdinCheck(Arc::new(check))));
        self
    }

    /// Appends more stdout lines to the script, typically after
    /// [`FakeTransport::wait_for_stdin`].
    pub fn then_stdout<I, S>(mut self, stdout: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.script
            .stdout
            .extend(stdout.into_iter().map(|l| Step::Line(l.into())));
        self
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

    /// What every failed [`FakeTransport::wait_for_stdin`] check said, in order.
    pub fn stub_failures(&self) -> Vec<String> {
        self.lock().failures.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Record> {
        lock(&self.record)
    }
}

fn lock(record: &Mutex<Record>) -> std::sync::MutexGuard<'_, Record> {
    // A poisoned lock means another test thread panicked while holding it;
    // the record itself is still usable.
    record.lock().unwrap_or_else(|e| e.into_inner())
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
            stdin_count: self.stdin_count.clone(),
            ended: ended.clone(),
            killed: killed.clone(),
        };
        let stdout = FakeReader {
            steps: self.script.stdout.clone().into(),
            ended: ended_rx.clone(),
            stdin: Some(StdinFeed {
                record: self.record.clone(),
                count: self.stdin_count.subscribe(),
                consumed: 0,
            }),
        };
        let stderr = FakeReader {
            steps: self.script.stderr.iter().cloned().map(Step::Line).collect(),
            // Stderr never holds open; it ends after its scripted lines.
            ended: watch::channel(true).1,
            stdin: None,
        };
        Ok(Box::new(FakeProcess {
            exit: ProcessExit {
                code: self.script.exit_code,
            },
            ended,
            stdin: Some(Box::new(stdin)),
            stdout: Some(Box::new(stdout)),
            stderr: Some(Box::new(stderr)),
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
    stdin_count: Arc<watch::Sender<usize>>,
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
        let count = {
            let mut record = lock(&self.record);
            record.stdin.push(line.to_string());
            record.stdin.len()
        };
        // Wakes a stdout paused in `wait_for_stdin`.
        self.stdin_count.send_replace(count);
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
    steps: VecDeque<Step>,
    ended: watch::Receiver<bool>,
    /// How a paused stdout sees stdin; `None` for stderr, which never pauses.
    stdin: Option<StdinFeed>,
}

/// A stdout's view of what has been written to stdin.
struct StdinFeed {
    record: Arc<Mutex<Record>>,
    count: watch::Receiver<usize>,
    /// How many stdin lines earlier `AwaitStdin` steps have consumed.
    consumed: usize,
}

#[async_trait]
impl LineReader for FakeReader {
    async fn next_line(&mut self) -> io::Result<Option<String>> {
        loop {
            match self.steps.pop_front() {
                // Taking a line happens in one step with no await before it,
                // so a dropped call can never lose one.
                Some(Step::Line(line)) => return Ok(Some(line)),
                Some(Step::AwaitStdin(check)) => {
                    // Put the step back while waiting: if this future is
                    // dropped mid-wait, the next call starts the wait again
                    // and nothing has been consumed.
                    self.steps.push_front(Step::AwaitStdin(check.clone()));
                    let feed = self
                        .stdin
                        .as_mut()
                        .expect("only a stdout script waits for stdin");
                    let wanted = feed.consumed + 1;
                    if feed.count.wait_for(|n| *n >= wanted).await.is_err() {
                        // The transport is gone, so no line can ever arrive.
                        self.steps.clear();
                        continue;
                    }
                    self.steps.pop_front();
                    let line = lock(&feed.record).stdin[feed.consumed].clone();
                    feed.consumed = wanted;
                    if let Err(failure) = (check.0)(&line) {
                        lock(&feed.record).failures.push(failure);
                        self.steps.clear();
                    }
                }
                None => {
                    wait_until_ended(&mut self.ended).await;
                    return Ok(None);
                }
            }
        }
    }
}
