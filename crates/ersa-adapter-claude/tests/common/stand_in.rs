//! A stand-in Claude Code CLI for the sign-in tests: it plays a script of
//! recorded or hand-written lines, answering the adapter's requests as the
//! recording's CLI answered the recorder's.
//!
//! The recorder named its requests `ersa-initialize-1`, `ersa-claude_authenticate-2`
//! and so on, and the adapter names them its own way, so the stand-in maps
//! each recorded request id to the id the adapter actually sent and writes
//! the recorded answers under the adapter's ids. Everything else is played
//! as recorded, the stdout lines read from the fixture files.
//!
//! Unlike `FakeTransport`, a process whose handle is dropped keeps running
//! here: only a kill, a closed stdin or the script's own exit ends it. So a
//! test that counts running processes catches an adapter that walks away
//! from one.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ersa_adapter::recording::Recording;
use ersa_adapter::{
    Direction, LineReader, LineWriter, Process, ProcessExit, ProcessSpec, Transport, async_trait,
};
use serde_json::Value;
use tokio::sync::watch;

/// How long a test waits on the stand-in before failing: generous, since a
/// busy machine is slow, and never a pause a test needs to pass.
const PATIENCE: Duration = Duration::from_secs(60);

/// One step of a stand-in's stdout.
#[derive(Debug, Clone)]
pub enum Step {
    /// Waits for the adapter's next stdin line and checks it is this one,
    /// its `request_id` aside.
    Expect(String),
    /// Writes this stdout line, a `control_response`'s `request_id` mapped
    /// to the adapter's own.
    Emit(String),
    /// Waits until the test calls [`StandIn::release`].
    Gate,
    /// Exits with this code, stdout ending.
    Exit(i32),
}

/// What one stand-in process plays.
#[derive(Debug, Clone, Default)]
pub struct Script {
    pub steps: Vec<Step>,
    pub stderr: Vec<String>,
}

impl Script {
    /// Every line of `recording`, in the order it crossed, up to the host's
    /// first `control_cancel_request`: stopping is #670's, and this ticket's
    /// stop only closes stdin, which ends the process (fixture
    /// 13-sign-in-closed-while-waiting).
    pub fn recorded(recording: &Recording) -> Script {
        let steps = recording
            .lines()
            .iter()
            .take_while(|line| {
                !(line.dir == Direction::Host
                    && parse(&line.line)["type"] == "control_cancel_request")
            })
            .map(|line| match line.dir {
                Direction::Host => Step::Expect(line.line.clone()),
                Direction::Backend => Step::Emit(line.line.clone()),
            })
            .collect();
        Script {
            steps,
            stderr: Vec::new(),
        }
    }

    /// The recorded id of the request whose subtype is `subtype`.
    fn request_id(&self, subtype: &str) -> String {
        self.steps
            .iter()
            .find_map(|step| match step {
                Step::Expect(line) if parse(line)["request"]["subtype"] == subtype => {
                    parse(line)["request_id"].as_str().map(String::from)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the script asks for no {subtype}"))
    }

    /// Where the step expecting the request `subtype` is.
    fn expect_at(&self, subtype: &str) -> usize {
        self.steps
            .iter()
            .position(|step| {
                matches!(step, Step::Expect(line) if parse(line)["request"]["subtype"] == subtype)
            })
            .unwrap_or_else(|| panic!("the script asks for no {subtype}"))
    }

    /// Where the step answering the request `subtype` is.
    fn answer_at(&self, subtype: &str) -> usize {
        let id = self.request_id(subtype);
        self.steps
            .iter()
            .position(|step| {
                matches!(step, Step::Emit(line) if parse(line)["response"]["request_id"] == id.as_str())
            })
            .unwrap_or_else(|| panic!("the script answers no {subtype}"))
    }

    /// The script up to and including the step expecting `subtype`.
    pub fn through_request(mut self, subtype: &str) -> Script {
        let at = self.expect_at(subtype);
        self.steps.truncate(at + 1);
        self
    }

    /// The script up to and including the answer to `subtype`.
    pub fn through_answer(mut self, subtype: &str) -> Script {
        let at = self.answer_at(subtype);
        self.steps.truncate(at + 1);
        self
    }

    /// The answer to `subtype`, its `response` object as `change` leaves it.
    pub fn answering(mut self, subtype: &str, change: impl FnOnce(&mut Value)) -> Script {
        let at = self.answer_at(subtype);
        let Step::Emit(line) = &self.steps[at] else {
            unreachable!("an answer is a stdout line")
        };
        let mut answer = parse(line);
        change(&mut answer["response"]);
        self.steps[at] = Step::Emit(answer.to_string());
        self
    }

    /// The recorded answer to `subtype`, its `response` object as `change`
    /// leaves it, as a line to put elsewhere in a script.
    pub fn answer(&self, subtype: &str, change: impl FnOnce(&mut Value)) -> String {
        let Step::Emit(line) = &self.steps[self.answer_at(subtype)] else {
            unreachable!("an answer is a stdout line")
        };
        let mut answer = parse(line);
        change(&mut answer["response"]);
        answer.to_string()
    }

    /// An error answer to `subtype` with `error` as its text, under the
    /// recorded id, so the stand-in sends it under the adapter's.
    pub fn error_answer(&self, subtype: &str, error: Value) -> String {
        serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "error",
                "request_id": self.request_id(subtype),
                "error": error,
            }
        })
        .to_string()
    }

    /// The steps before the answer to `subtype`, `steps`, then the answer
    /// and the rest.
    pub fn before_answer(mut self, subtype: &str, steps: impl IntoIterator<Item = Step>) -> Script {
        let at = self.answer_at(subtype);
        self.steps.splice(at..at, steps);
        self
    }

    /// The steps through the answer to `subtype`, `steps`, then the rest.
    pub fn after_answer(mut self, subtype: &str, steps: impl IntoIterator<Item = Step>) -> Script {
        let at = self.answer_at(subtype) + 1;
        self.steps.splice(at..at, steps);
        self
    }

    /// The steps through the one expecting `subtype`, `steps`, then the rest.
    pub fn after_request(mut self, subtype: &str, steps: impl IntoIterator<Item = Step>) -> Script {
        let at = self.expect_at(subtype) + 1;
        self.steps.splice(at..at, steps);
        self
    }

    /// The script, then `steps`.
    pub fn then(mut self, steps: impl IntoIterator<Item = Step>) -> Script {
        self.steps.extend(steps);
        self
    }

    /// Also writes these lines to stderr when the process starts.
    pub fn with_stderr(mut self, lines: impl IntoIterator<Item = String>) -> Script {
        self.stderr.extend(lines);
        self
    }

    /// The host lines this script expects, in order.
    pub fn expected(&self) -> Vec<String> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                Step::Expect(line) => Some(line.clone()),
                _ => None,
            })
            .collect()
    }
}

pub fn parse(line: &str) -> Value {
    serde_json::from_str(line).unwrap_or(Value::Null)
}

/// A host line with its `request_id` taken out, to compare with a
/// recorded one.
pub fn without_id(line: &str) -> Value {
    let mut value = parse(line);
    if let Some(object) = value.as_object_mut() {
        object.remove("request_id");
    }
    value
}

/// A transport whose processes each play the next of its scripts.
#[derive(Clone, Default)]
pub struct StandIn {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    scripts: VecDeque<Script>,
    processes: Vec<(ProcessSpec, Arc<Shared>)>,
}

/// One process's state, shared by its handle, its streams and the test.
struct Shared {
    state: Mutex<State>,
    /// Bumped on every change, so whoever waits looks again.
    changed: watch::Sender<u64>,
}

#[derive(Default)]
struct State {
    /// Every line the adapter wrote, in order.
    host_lines: Vec<String>,
    /// Lines the script has not taken yet.
    unread: VecDeque<String>,
    /// Recorded request id to the adapter's.
    ids: HashMap<String, String>,
    exit: Option<ProcessExit>,
    /// The stdout reader is waiting on the adapter, a gate or the end.
    waiting: bool,
    gates_open: usize,
    failures: Vec<String>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn touch(&self) {
        self.changed.send_modify(|n| *n += 1);
    }

    fn end(&self, code: Option<i32>) {
        let mut state = self.lock();
        if state.exit.is_none() {
            state.exit = Some(ProcessExit { code });
        }
        drop(state);
        self.touch();
    }

    /// Waits until `done` holds, failing the test after [`PATIENCE`].
    async fn until(&self, what: &str, done: impl Fn(&State) -> bool) {
        let mut changed = self.changed.subscribe();
        let waited = tokio::time::timeout(PATIENCE, async {
            while !done(&self.lock()) {
                changed.changed().await.expect("the stand-in is alive");
            }
        })
        .await;
        if waited.is_err() {
            let state = self.lock();
            panic!(
                "the stand-in was not {what} within {PATIENCE:?}: waiting {}, {} lines unread, \n                 failures {:#?}; it was sent {:#?}",
                state.waiting,
                state.unread.len(),
                state.failures,
                state.host_lines
            );
        }
    }
}

impl StandIn {
    /// A stand-in whose processes play `scripts`, one each, in order.
    pub fn new(scripts: impl IntoIterator<Item = Script>) -> StandIn {
        let stand_in = StandIn::default();
        stand_in.lock().scripts = scripts.into_iter().collect();
        stand_in
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn process(&self, n: usize) -> Arc<Shared> {
        let processes = &self.lock().processes;
        let (_, shared) = processes
            .get(n)
            .unwrap_or_else(|| panic!("the stand-in started {} processes", processes.len()));
        Arc::clone(shared)
    }

    /// Every spec the adapter started a process with, in order.
    pub fn spawned(&self) -> Vec<ProcessSpec> {
        self.lock()
            .processes
            .iter()
            .map(|(spec, _)| spec.clone())
            .collect()
    }

    /// The lines the adapter wrote to process `n`'s stdin.
    pub fn host_lines(&self, n: usize) -> Vec<String> {
        self.process(n).lock().host_lines.clone()
    }

    /// Every line any process was sent that its script did not expect.
    pub fn failures(&self) -> Vec<String> {
        let processes: Vec<Arc<Shared>> = self
            .lock()
            .processes
            .iter()
            .map(|(_, s)| Arc::clone(s))
            .collect();
        processes
            .iter()
            .flat_map(|shared| shared.lock().failures.clone())
            .collect()
    }

    /// How many of its processes are running.
    pub fn running(&self) -> usize {
        let processes: Vec<Arc<Shared>> = self
            .lock()
            .processes
            .iter()
            .map(|(_, s)| Arc::clone(s))
            .collect();
        processes
            .iter()
            .filter(|shared| shared.lock().exit.is_none())
            .count()
    }

    /// How process `n` exited, once it has.
    pub fn exit(&self, n: usize) -> Option<ProcessExit> {
        self.process(n).lock().exit
    }

    /// Opens the next gate in process `n`'s script.
    pub fn release(&self, n: usize) {
        let shared = self.process(n);
        let mut state = shared.lock();
        state.gates_open += 1;
        // Not waiting any more, as far as a test can tell, until the reader
        // has passed the gate and handed over what follows it.
        state.waiting = false;
        drop(state);
        shared.touch();
    }

    /// Waits until process `n` has taken every line the adapter wrote and
    /// is waiting on the adapter, a gate or the end of its script, or has
    /// exited. The adapter's read loop handles each line before it asks
    /// for the next, so by then it has handled every line written so far.
    pub async fn settled(&self, n: usize) {
        self.process(n)
            .until("settled", |state| {
                state.exit.is_some() || (state.waiting && state.unread.is_empty())
            })
            .await;
    }

    /// Waits until process `n` has exited.
    pub async fn exited(&self, n: usize) {
        self.process(n)
            .until("exited", |state| state.exit.is_some())
            .await;
    }
}

#[async_trait]
impl Transport for StandIn {
    async fn spawn(&self, spec: ProcessSpec) -> io::Result<Box<dyn Process>> {
        let mut inner = self.lock();
        let script = inner
            .scripts
            .pop_front()
            .ok_or_else(|| io::Error::other("the stand-in has no script for another process"))?;
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            changed: watch::channel(0).0,
        });
        inner.processes.push((spec, Arc::clone(&shared)));
        Ok(Box::new(StandInProcess {
            stdin: Some(Box::new(Stdin(Arc::clone(&shared)))),
            stdout: Some(Box::new(Stdout {
                shared: Arc::clone(&shared),
                steps: script.steps.into(),
            })),
            stderr: Some(Box::new(Stderr(script.stderr.into()))),
            shared,
        }))
    }
}

struct StandInProcess {
    shared: Arc<Shared>,
    stdin: Option<Box<dyn LineWriter>>,
    stdout: Option<Box<dyn LineReader>>,
    stderr: Option<Box<dyn LineReader>>,
}

#[async_trait]
impl Process for StandInProcess {
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
        let mut changed = self.shared.changed.subscribe();
        loop {
            if let Some(exit) = self.shared.lock().exit {
                return Ok(exit);
            }
            changed.changed().await.expect("the stand-in is alive");
        }
    }

    async fn kill(&mut self) -> io::Result<()> {
        // A killed process has no exit code, as on Unix after a signal.
        self.shared.end(None);
        Ok(())
    }
}

struct Stdin(Arc<Shared>);

#[async_trait]
impl LineWriter for Stdin {
    async fn write_line(&mut self, line: &str) -> io::Result<()> {
        let mut state = self.0.lock();
        if state.exit.is_some() {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        state.host_lines.push(line.to_string());
        state.unread.push_back(line.to_string());
        drop(state);
        self.0.touch();
        Ok(())
    }
}

/// Closing stdin ends the process with exit 0, as it ends the recorded CLI
/// whether or not a wait is pending (fixtures 10 and 13).
impl Drop for Stdin {
    fn drop(&mut self) {
        self.0.end(Some(0));
    }
}

struct Stdout {
    shared: Arc<Shared>,
    steps: VecDeque<Step>,
}

impl Stdout {
    /// The next line to hand over, `Ok(None)` at the end of stdout, or
    /// `Err(newly)` when the script has to wait, `newly` when it was not
    /// waiting before. Everything happens under the
    /// lock and nothing waits in here, so a read dropped part way loses
    /// nothing.
    fn step(&mut self) -> Result<Option<String>, bool> {
        let mut state = self.shared.lock();
        loop {
            if state.exit.is_some() {
                return Ok(None);
            }
            match self.steps.front() {
                None => {
                    return Err(!std::mem::replace(&mut state.waiting, true));
                }
                Some(Step::Emit(line)) => {
                    let line = mapped(line, &state.ids);
                    self.steps.pop_front();
                    state.waiting = false;
                    return Ok(Some(line));
                }
                Some(Step::Expect(wanted)) => {
                    let Some(got) = state.unread.pop_front() else {
                        return Err(!std::mem::replace(&mut state.waiting, true));
                    };
                    // Busy again, so the next wait is announced.
                    state.waiting = false;
                    if without_id(&got) == without_id(wanted) {
                        if let (Some(recorded), Some(actual)) = (
                            parse(wanted)["request_id"].as_str(),
                            parse(&got)["request_id"].as_str(),
                        ) {
                            state.ids.insert(recorded.to_string(), actual.to_string());
                        }
                        self.steps.pop_front();
                    } else {
                        // A real CLI sent a line it did not expect would not
                        // answer as recorded: say so, and play nothing more.
                        state
                            .failures
                            .push(format!("expected {wanted}\n     got {got}"));
                        self.steps.clear();
                    }
                }
                Some(Step::Gate) => {
                    if state.gates_open == 0 {
                        return Err(!std::mem::replace(&mut state.waiting, true));
                    }
                    state.gates_open -= 1;
                    state.waiting = false;
                    self.steps.pop_front();
                }
                Some(Step::Exit(code)) => {
                    state.exit = Some(ProcessExit { code: Some(*code) });
                    drop(state);
                    self.shared.touch();
                    return Ok(None);
                }
            }
        }
    }
}

#[async_trait]
impl LineReader for Stdout {
    async fn next_line(&mut self) -> io::Result<Option<String>> {
        let mut changed = self.shared.changed.subscribe();
        loop {
            match self.step() {
                Ok(line) => return Ok(line),
                Err(newly) => {
                    // Seen as waiting from here: say so to whoever waits on
                    // it, once, so the reader does not wake itself.
                    if newly {
                        self.shared.touch();
                    }
                    changed.changed().await.expect("the stand-in is alive");
                }
            }
        }
    }
}

/// A `control_response`'s `request_id` as the adapter named that request.
fn mapped(line: &str, ids: &HashMap<String, String>) -> String {
    let mut value = parse(line);
    let Some(id) = value["response"]["request_id"].as_str() else {
        return line.to_string();
    };
    match ids.get(id) {
        Some(actual) => {
            value["response"]["request_id"] = Value::String(actual.clone());
            value.to_string()
        }
        None => line.to_string(),
    }
}

struct Stderr(VecDeque<String>);

#[async_trait]
impl LineReader for Stderr {
    async fn next_line(&mut self) -> io::Result<Option<String>> {
        Ok(self.0.pop_front())
    }
}
