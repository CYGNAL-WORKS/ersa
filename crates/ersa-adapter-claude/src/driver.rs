//! Starting and driving one Claude Code CLI process per session.
//!
//! One task owns the process: it reads stdout line by line, hands each line to
//! the sink, routes control responses to whoever is waiting for them, watches
//! for the process to exit, and finally reports the exit. The session methods
//! write to stdin from whatever task Core calls them on, through one lock,
//! recording each line before it is written (PROTO-CLAUDE-CODE-STREAM-R07; the
//! transport flushes every line).
//!
//! Nothing here may wait forever on the CLI. A CLI can stop reading stdin (a
//! write then blocks once the pipe fills), and on Windows a process the agent
//! started can inherit the stdout handle and keep it open after the CLI has
//! gone (stdout then never ends). So the exit is detected from the process
//! itself, writes give up when the session is killed, and `stop` kills after
//! its grace period whatever else is going on.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, BackendChoices, Direction, LineReader,
    LineWriter, PermissionDecision, Process, ProcessExit, ProcessSpec, Projector, RecordEvent,
    RecordSummary, SessionConfig, Transport, async_trait,
};
use ersa_protocol::core_api::Choice;
use ersa_protocol::events::PermissionBehavior;
use serde_json::{Value, json};
use tokio::sync::{oneshot, watch};

use crate::projector::ClaudeProjector;
use crate::record;
use crate::suggestions;
use crate::wire::{
    HostControlResponse, HostMessage, HostRequest, HostResponse, InitializeRequest, WorkspaceTrust,
};

/// After the process has exited, how long stdout may stay quiet before the
/// read loop stops waiting for the rest of it. Lines the CLI wrote before
/// exiting arrive at once; silence this long means someone else holds the pipe.
const DRAIN_IDLE: Duration = Duration::from_millis(500);

/// And the longest the read loop drains stdout after the exit, however busy it
/// stays: whatever still writes to it is not the CLI.
const DRAIN_MAX: Duration = Duration::from_secs(5);

/// How many trailing stderr lines a failed start quotes.
const STDERR_TAIL: usize = 5;

/// The Claude Code adapter (backend `claude-code`).
#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    handshake_timeout: Duration,
    stop_grace: Duration,
    /// Where the CLI keeps session records; `None` for wherever the CLI
    /// itself would ([`record::default_claude_home`]).
    claude_home: Option<PathBuf>,
    /// The records read last, reused while unchanged.
    records: Arc<record::RecordCache>,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        ClaudeAdapter {
            // A cold CLI start took about four seconds in the recordings;
            // this leaves plenty of room for a slow disk or a remote host.
            handshake_timeout: Duration::from_secs(60),
            stop_grace: Duration::from_secs(3),
            claude_home: None,
            records: Arc::default(),
        }
    }

    /// Reads session records under `claude_home` instead of the CLI's own
    /// default, for tests with a scratch home.
    pub fn with_claude_home(mut self, claude_home: PathBuf) -> Self {
        self.claude_home = Some(claude_home);
        self
    }

    /// How long `start` waits for the CLI to answer `initialize`.
    pub fn with_handshake_timeout(mut self, timeout: Duration) -> Self {
        self.handshake_timeout = timeout;
        self
    }

    /// How long `stop` waits for the CLI to exit after stdin closes before
    /// killing it.
    pub fn with_stop_grace(mut self, grace: Duration) -> Self {
        self.stop_grace = grace;
        self
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        ClaudeAdapter::new()
    }
}

/// The CLI's arguments for a session, every behaviour-affecting flag spelled
/// out (PROTO-CLAUDE-CODE-STREAM-R01, ADR 0014).
pub fn invocation_args(config: &SessionConfig) -> Vec<String> {
    // An empty mode would leave the choice to the CLI; `default` says it.
    let mode = if config.permission_mode.is_empty() {
        "default"
    } else {
        &config.permission_mode
    };
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--permission-mode",
        mode,
        "--setting-sources",
        setting_sources(config),
        // Permission prompts arrive as `can_use_tool` control requests.
        "--permission-prompt-tool",
        "stdio",
        // Read `initialize` before starting rather than racing it.
        "--await-initialize",
        // Token-level `stream_event` lines, so replies stream.
        "--include-partial-messages",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    // Named by the session, or left out so the CLI's own defaults apply.
    if let Some(model) = &config.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(effort) = &config.effort {
        args.push("--effort".into());
        args.push(effort.clone());
    }
    args
}

/// The models, efforts and permission modes `backend.list` offers for
/// Claude Code (PROTO-CLAUDE-CODE-STREAM, Invocation). The defaults are what
/// the CLI starts with when no flag names one: 2.1.316 sessions started
/// without `--model` or `--effort` ran Opus at medium effort (the 2.1.316
/// session-record fixtures). `default` asks before every action outside the
/// allow rules (FLOW-NEW-SESSION-R02). `bypassPermissions` is not offered.
fn choices() -> BackendChoices {
    let list = |items: &[(&str, &str)], default: &str| -> Vec<Choice> {
        items
            .iter()
            .map(|&(id, label)| Choice {
                id: id.into(),
                label: label.into(),
                default: id == default,
            })
            .collect()
    };
    BackendChoices {
        models: list(
            &[
                ("fable", "Fable"),
                ("opus", "Opus"),
                ("sonnet", "Sonnet"),
                ("haiku", "Haiku"),
            ],
            "opus",
        ),
        efforts: list(
            &[
                ("low", "Low"),
                ("medium", "Medium"),
                ("high", "High"),
                ("xhigh", "Extra high"),
                ("max", "Max"),
            ],
            "medium",
        ),
        permission_modes: list(
            &[
                ("default", "Ask before acting"),
                ("acceptEdits", "Accept edits"),
                ("plan", "Plan"),
                ("auto", "Auto"),
            ],
            "default",
        ),
    }
}

/// The attestation `initialize` carries for a trusted project's session: the
/// user accepted Ersa's trust question for the project's root, so the CLI may
/// record it as trusted in its own configuration, which Ersa never writes
/// (ADR 0018). The CLI keys trust by git root, so the record covers every
/// linked worktree of the project. An untrusted project's session, or one
/// whose root is unknown, attests nothing.
fn workspace_trust(config: &SessionConfig) -> Option<WorkspaceTrust> {
    (config.project_trusted && !config.project_path.is_empty()).then(|| WorkspaceTrust {
        accepted: true,
        directory: config.project_path.clone(),
    })
}

/// Which settings files the CLI loads. A trusted project's session loads the
/// user's, the project's and the local settings, and (with no
/// `--strict-mcp-config`) the user's and the project's MCP servers, as the VS
/// Code extension does: an ordinary Claude Code session (ADR 0017,
/// PROTO-CLAUDE-CODE-STREAM-R12).
///
/// An untrusted project's session loads the user's settings only. With
/// `project` and `local` left out, the CLI reads none of the workspace's
/// `.claude/settings*.json` (so none of its hooks, rules or permission mode)
/// and starts none of its `.mcp.json` servers, even when the user's settings
/// enable them, while the user's own settings and MCP servers still load
/// (observed on 2.1.316; PROTO-CLAUDE-CODE-STREAM-R18, ADR 0018). Headless,
/// the CLI never asks whether to trust a folder, so this is the only gate.
fn setting_sources(config: &SessionConfig) -> &'static str {
    if config.project_trusted {
        "user,project,local"
    } else {
        "user"
    }
}

/// The child's environment: Ersa names itself as the entrypoint, so the CLI
/// lists its sessions in the terminal picker and VS Code rather than hiding
/// them as SDK sessions (PROTO-CLAUDE-CODE-STREAM-R13, ADR 0017).
fn entrypoint_env() -> BTreeMap<String, String> {
    BTreeMap::from([("CLAUDE_CODE_ENTRYPOINT".into(), "ersa".into())])
}

/// Variables that would carry whatever started Ersa into the session: a
/// Claude Code (or Claude Desktop) parent marks its children as nested
/// sessions, Node options change how the CLI runs, and trace context would
/// file the session under the parent's trace (PROTO-CLAUDE-CODE-STREAM-R13).
/// `CLAUDE_CODE_ENTRYPOINT` is removed too and then set by `entrypoint_env`:
/// transports apply removals first, so the parent's value never reaches the
/// child.
///
/// The rest tie a process to another host's session, seen when Ersa is
/// started from inside Claude Desktop or a VS Code terminal: the session it
/// belongs to, the host's messaging channel and IDE connection, the SDK that
/// started it (PROTO-CLAUDE-CODE-STREAM-R19). Variables that configure
/// Claude Code itself (its provider, limits, configuration folder) stay, as
/// they do in the user's terminal.
const INHERITED_VARIABLES: [&str; 13] = [
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_ENTRYPOINT",
    "NODE_OPTIONS",
    "TRACEPARENT",
    "TRACESTATE",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SSE_PORT",
    "ENABLE_IDE_INTEGRATION",
    "CLAUDE_AGENT_SDK_VERSION",
];

/// Which `claude` to run: `extra.claude_path` from the session config, else the
/// `ERSA_CLAUDE` environment variable, else whatever `claude` the host finds.
/// An empty value at either step counts as unset.
fn program(config: &SessionConfig) -> String {
    let configured = config
        .extra
        .get("claude_path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(String::from);
    let from_env = || {
        std::env::var("ERSA_CLAUDE")
            .ok()
            .filter(|path| !path.is_empty())
    };
    configured
        .or_else(from_env)
        .unwrap_or_else(|| "claude".into())
}

/// The arguments that continue an earlier session, after
/// [`invocation_args`]: `--resume` with `extra.resume`, the `session_id` from
/// that session's `system/init`, which Ersa stores with the session. The CLI
/// keeps that id and appends to its session record
/// (PROTO-CLAUDE-CODE-STREAM-R14). None for a new session (`extra.resume`
/// absent or `null`).
///
/// The CLI's session ids are UUIDs, and anything else is an error, so a value
/// that merely looks like a flag (from a file name in the record directory,
/// say) can never reach the command line as one.
pub fn resume_args(config: &SessionConfig) -> Result<Vec<String>, AdapterError> {
    let Some(value) = config.extra.get("resume").filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    match value.as_str() {
        Some(id) if crate::record::is_session_id(id) => Ok(vec!["--resume".into(), id.into()]),
        _ => Err(AdapterError::Protocol(format!(
            "cannot resume {value}: not a Claude Code session id"
        ))),
    }
}

#[async_trait]
impl Adapter for ClaudeAdapter {
    fn backend(&self) -> &str {
        crate::BACKEND
    }

    fn label(&self) -> &str {
        "Claude Code"
    }

    fn tested_versions(&self) -> Option<(&'static str, &'static str)> {
        Some((crate::MIN_TESTED_VERSION, crate::MAX_TESTED_VERSION))
    }

    fn choices(&self) -> BackendChoices {
        choices()
    }

    fn set_environment(&self, spec: &mut ProcessSpec) {
        spec.env_remove
            .extend(INHERITED_VARIABLES.map(String::from));
        spec.env.extend(entrypoint_env());
    }

    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        // A bad resume id fails here, before anything starts.
        let resume = resume_args(&config)?;

        let mut spec = ProcessSpec {
            program: program(&config),
            args: invocation_args(&config),
            cwd: (!config.cwd.is_empty()).then(|| config.cwd.clone()),
            ..ProcessSpec::default()
        };
        self.set_environment(&mut spec);
        spec.args.extend(resume);
        let program = spec.program.clone();
        let mut process = transport.spawn(spec).await.map_err(|e| {
            // Name what was tried: "program not found" alone does not say
            // which program, or where to set another.
            AdapterError::Io(io::Error::new(
                e.kind(),
                format!(
                    "could not start Claude Code as `{program}` ({e}); \
                     set ERSA_CLAUDE or the session's claude_path to the CLI"
                ),
            ))
        })?;
        let stdin = process.take_stdin().ok_or(AdapterError::NotRunning)?;
        let stdout = process.take_stdout().ok_or(AdapterError::NotRunning)?;

        let (exited_tx, exited_rx) = watch::channel(false);
        let shared = Arc::new(Shared {
            sink,
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            stopping: AtomicBool::new(false),
            kill: watch::channel(false).0,
            awaiting: Mutex::default(),
            permissions: Mutex::default(),
            deciding: Mutex::default(),
            stderr_tail: Mutex::default(),
            next_id: AtomicU64::new(1),
        });
        let stderr = process
            .take_stderr()
            .map(|stderr| tokio::spawn(drain_stderr(stderr, shared.clone())));
        let session = ClaudeSession {
            shared: shared.clone(),
            exited: exited_rx,
            stop_grace: self.stop_grace,
        };

        // Register for the answer before the request can be written, so the
        // read loop always finds someone waiting for it.
        let init_id = shared.request_id("init");
        let (answer_tx, answer_rx) = oneshot::channel();
        lock(&shared.awaiting).insert(init_id.clone(), answer_tx);
        tokio::spawn(read_loop(shared.clone(), stdout, process, exited_tx));

        // The handshake (PROTO-CLAUDE-CODE-STREAM-R02): no user message goes
        // out until the CLI has answered `initialize`, and `start` returns only
        // then, so the answer is in the raw log before anything Core sends.
        let trust = workspace_trust(&config);
        let attested = trust.is_some();
        let request = HostRequest::Initialize(Box::new(InitializeRequest {
            workspace_trust: trust,
            ..InitializeRequest::default()
        }));
        let handshake = async {
            // Failing to send it is as much a failed start as a bad answer,
            // and the CLI's stderr usually says why.
            shared
                .write(&HostMessage::control_request(&init_id, request))
                .await
                .map_err(|e| AdapterError::Protocol(format!("could not send initialize: {e}")))?;
            match answer_rx.await {
                Ok(answer) if answer["subtype"] == "success" => {
                    // A trust the CLI did not record leaves the workspace's
                    // always-allow rules ignored, but the session runs.
                    if attested && answer["response"]["workspace_trust_recorded"] != true {
                        tracing::warn!(
                            directory = %config.project_path,
                            recorded = %answer["response"]["workspace_trust_recorded"],
                            "Claude Code did not record the trusted project's workspace trust"
                        );
                    }
                    Ok(())
                }
                Ok(answer) => Err(AdapterError::Protocol(format!(
                    "the CLI refused initialize: {}",
                    answer["error"].as_str().unwrap_or("no reason given")
                ))),
                // The read loop dropped the sender: the CLI exited first.
                Err(_) => Err(AdapterError::Protocol(
                    "the CLI exited before answering initialize".into(),
                )),
            }
        };
        // The timeout covers the write too: a CLI that never reads its stdin
        // would otherwise hold `start` forever.
        let handshake = async {
            tokio::time::timeout(self.handshake_timeout, handshake)
                .await
                .unwrap_or_else(|_| {
                    Err(AdapterError::Protocol(format!(
                        "the CLI did not answer initialize within {:?}",
                        self.handshake_timeout
                    )))
                })
        };
        match handshake.await {
            Ok(()) => Ok(Box::new(session)),
            Err(error) => {
                // A half-started CLI is of no use; end it before reporting.
                session.kill_and_wait().await;
                // What the CLI said on its way out is usually the real reason.
                if let Some(stderr) = stderr {
                    let _ = tokio::time::timeout(DRAIN_IDLE, stderr).await;
                }
                Err(with_stderr(error, &shared.stderr_tail()))
            }
        }
    }

    fn projector(&self) -> Box<dyn Projector> {
        Box::new(ClaudeProjector::new())
    }

    /// The session's record, read from this machine's files: the transport
    /// has no file access yet, so a remote host's record is not read (M4).
    async fn read_record(
        &self,
        _transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Arc<Vec<RecordEvent>>, AdapterError> {
        let home = self.claude_home()?;
        let (cwd, id) = (cwd.to_string(), backend_session_id.to_string());
        // Reading a long record takes a while; off the async threads.
        let records = Arc::clone(&self.records);
        blocking(move || records.read(&home, &cwd, &id)).await
    }

    /// The record file's size and modified time.
    async fn record_version(
        &self,
        _transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Option<String>, AdapterError> {
        let home = self.claude_home()?;
        let (cwd, id) = (cwd.to_string(), backend_session_id.to_string());
        blocking(move || record::record_version(&home, &cwd, &id)).await
    }

    fn forget_record(&self, _cwd: &str, backend_session_id: &str) {
        self.records.forget(backend_session_id);
    }

    /// The record's title and first time, from its head and tail
    /// (PROTO-CLAUDE-CODE-STREAM-R16), read from this machine's files.
    async fn summarize_record(
        &self,
        _transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Option<RecordSummary>, AdapterError> {
        let home = self.claude_home()?;
        let (cwd, id) = (cwd.to_string(), backend_session_id.to_string());
        let records = Arc::clone(&self.records);
        let summary = blocking(move || records.summarize(&home, &cwd, &id)).await?;
        Ok(summary.map(|summary| RecordSummary {
            title: summary.title,
            created_at: summary.created_at,
        }))
    }
}

impl ClaudeAdapter {
    /// Where the CLI keeps its records: the one set for tests, else the CLI's
    /// own default.
    fn claude_home(&self) -> Result<PathBuf, AdapterError> {
        self.claude_home
            .clone()
            .or_else(record::default_claude_home)
            .ok_or_else(|| {
                AdapterError::Io(io::Error::new(
                    io::ErrorKind::NotFound,
                    "no home directory to find Claude Code's session records in",
                ))
            })
    }
}

/// Runs file reads off the async threads.
async fn blocking<T: Send + 'static>(
    read: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> Result<T, AdapterError> {
    tokio::task::spawn_blocking(read)
        .await
        .map_err(|e| AdapterError::Protocol(format!("reading the session record failed: {e}")))?
        .map_err(AdapterError::Io)
}

/// Adds the CLI's last stderr lines to a protocol error's message.
fn with_stderr(error: AdapterError, tail: &[String]) -> AdapterError {
    match error {
        AdapterError::Protocol(message) if !tail.is_empty() => {
            AdapterError::Protocol(format!("{message}; its stderr ended: {}", tail.join(" | ")))
        }
        other => other,
    }
}

/// What the read loop and the session methods share.
struct Shared {
    sink: Arc<dyn AdapterSink>,
    /// `None` once stdin is closed, by `stop` or because the process exited.
    stdin: tokio::sync::Mutex<Option<Box<dyn LineWriter>>>,
    /// Set by `stop`: no new line may be written.
    stopping: AtomicBool,
    /// Raised to kill the process. The read loop kills it; a write in progress
    /// or waiting for the stdin lock gives up.
    kill: watch::Sender<bool>,
    /// Host control requests waiting for the CLI's answer (the `response`
    /// object of its `control_response`), by `request_id`.
    awaiting: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    /// `can_use_tool` requests the CLI is waiting on, by `request_id`: the
    /// `request` object as raw JSON, so a request whose shape has drifted can
    /// still be answered rather than left to stall the turn.
    permissions: Mutex<HashMap<String, Value>>,
    /// Prompts whose answer is being written right now, so a second decision
    /// for the same prompt is refused instead of written too.
    deciding: Mutex<HashSet<String>>,
    /// The CLI's last few stderr lines, for a failed start's error.
    stderr_tail: Mutex<VecDeque<String>>,
    next_id: AtomicU64,
}

impl Shared {
    /// A fresh `request_id` for a host control request, such as `ersa-init-1`.
    fn request_id(&self, what: &str) -> String {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        format!("ersa-{what}-{n}")
    }

    fn stderr_tail(&self) -> Vec<String> {
        lock(&self.stderr_tail).iter().cloned().collect()
    }

    /// Records `message` as a host line, then writes it.
    ///
    /// The stdin lock is held across both, so lines are recorded in the order
    /// they are written even when several tasks write at once. Waiting for the
    /// lock and the write itself both give up when the session is killed, so a
    /// CLI that stopped reading cannot hold a caller forever.
    async fn write(&self, message: &HostMessage) -> Result<(), AdapterError> {
        let line = message
            .to_line()
            .map_err(|e| AdapterError::Protocol(format!("could not serialize a host line: {e}")))?;
        if self.stopping.load(Ordering::SeqCst) {
            return Err(AdapterError::NotRunning);
        }
        let mut kill = self.kill.subscribe();
        let mut stdin = tokio::select! {
            stdin = self.stdin.lock() => stdin,
            () = raised(&mut kill) => return Err(AdapterError::NotRunning),
        };
        let writer = stdin.as_mut().ok_or(AdapterError::NotRunning)?;
        self.sink.record(Direction::Host, line.clone());
        tokio::select! {
            written = writer.write_line(&line) => Ok(written?),
            () = raised(&mut kill) => Err(AdapterError::NotRunning),
        }
    }

    /// Handles one stdout line: records it, and keeps the driver's view of
    /// pending requests in step with it.
    async fn on_backend_line(&self, line: String) {
        // Read as raw JSON: the driver needs a handful of fields, and reading
        // them directly keeps working when the CLI's shapes drift. A line that
        // is not JSON is still recorded; the projector reports it.
        let message: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let field = |pointer: &str| message.pointer(pointer).and_then(Value::as_str);

        // Pending permission requests change before the line is recorded, so a
        // decision Core makes as soon as it sees the line finds the request.
        let mut unsupported = None;
        match (field("/type"), field("/request_id")) {
            (Some("control_request"), Some(id)) => match field("/request/subtype") {
                Some("can_use_tool") => {
                    lock(&self.permissions).insert(id.to_string(), message["request"].clone());
                }
                subtype => {
                    unsupported = Some((id.to_string(), subtype.unwrap_or("").to_string()));
                }
            },
            // The CLI withdrew a prompt (after an interrupt, say); a late
            // decision for it is refused rather than written.
            (Some("control_cancel_request"), Some(id)) => {
                lock(&self.permissions).remove(id);
            }
            _ => {}
        }
        let answered = match field("/type") {
            Some("control_response") => field("/response/request_id").map(String::from),
            _ => None,
        };

        self.sink.record(Direction::Backend, line);

        // Answers are delivered after recording, so whoever waits on one (the
        // handshake in `start`) resumes with the answer already in the log.
        if let Some(id) = answered
            && let Some(waiting) = lock(&self.awaiting).remove(&id)
        {
            // The waiter may have given up (timed out); nothing to do then.
            let _ = waiting.send(message["response"].clone());
        }

        // The CLI waits for an answer to every control request it sends, and
        // an unanswered one stalls the turn. Ersa only declares what it
        // handles, so this should not happen; if it does, say no rather than
        // leave the CLI hanging. The write gives up if the session is killed,
        // so a wedged CLI cannot stall this loop.
        if let Some((request_id, subtype)) = unsupported {
            let refusal = HostMessage::ControlResponse(HostControlResponse {
                response: HostResponse::Error {
                    request_id,
                    error: format!("Ersa does not handle {subtype} control requests"),
                },
            });
            // A failed write means the process is going away; the exit says so.
            let _ = self.write(&refusal).await;
        }
    }
}

/// Returns once the flag is raised. Wraps `wait_for` so `select!` sees a
/// future of `()`: its borrow of the flag's value never outlives the poll,
/// which keeps the futures that use it `Send`.
async fn raised(flag: &mut watch::Receiver<bool>) {
    // Fails only if the sender is gone, in which case nothing can raise it.
    if flag.wait_for(|raised| *raised).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Locks a std mutex, carrying on if a panicking thread poisoned it: every map
/// here stays consistent between single insertions and removals.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The task that owns the process. It reads stdout until the process exits
/// (or stdout ends), then reports the exit, exactly once and after the last
/// record.
async fn read_loop(
    shared: Arc<Shared>,
    mut stdout: Box<dyn LineReader>,
    mut process: Box<dyn Process>,
    exited: watch::Sender<bool>,
) {
    // What woke the loop. `select!` only picks; the work happens after it,
    // once the other branches' futures are gone.
    enum Next {
        Line(io::Result<Option<String>>),
        Exited(io::Result<ProcessExit>),
        Kill,
    }

    let mut kill = shared.kill.subscribe();
    let mut killed = false;
    let mut exit = None;
    loop {
        let next = tokio::select! {
            line = stdout.next_line() => Next::Line(line),
            // Exit is watched directly, not inferred from stdout ending: a
            // process the agent started may hold stdout open after the CLI
            // has gone.
            status = process.wait() => Next::Exited(status),
            () = raised(&mut kill), if !killed => Next::Kill,
        };
        match next {
            Next::Line(Ok(Some(line))) => shared.on_backend_line(line).await,
            // The transport decodes lossily, so an error here is the pipe
            // itself failing; nothing more will come from it.
            Next::Line(Ok(None) | Err(_)) => break,
            Next::Exited(status) => {
                exit = Some(status);
                break;
            }
            // `stop` ran out of patience, the start failed, or the session was
            // dropped. The exit follows and is handled above.
            Next::Kill => {
                let _ = process.kill().await;
                killed = true;
            }
        }
    }

    let exit = match exit {
        Some(status) => {
            // The CLI is gone, but lines it wrote just before exiting may still
            // be in the pipe, and every line belongs in the log. Read until the
            // pipe ends or goes quiet; whoever keeps it open is not the CLI.
            let deadline = tokio::time::Instant::now() + DRAIN_MAX;
            while let Ok(Ok(Some(line))) = tokio::time::timeout_at(
                deadline.min(tokio::time::Instant::now() + DRAIN_IDLE),
                stdout.next_line(),
            )
            .await
            {
                shared.on_backend_line(line).await;
            }
            status
        }
        // Stdout ended first; wait for the exit, killing if asked to.
        None => {
            let killed_while_waiting = tokio::select! {
                status = process.wait() => Err(status),
                () = raised(&mut kill), if !killed => Ok(()),
            };
            match killed_while_waiting {
                Err(status) => status,
                Ok(()) => {
                    let _ = process.kill().await;
                    process.wait().await
                }
            }
        }
    };

    // No line can be written now: refuse new writes, and make any write still
    // in progress give up (raising the kill flag after an exit is harmless).
    // Then close stdin under its lock, so no host line can be recorded after
    // the exit is reported.
    shared.stopping.store(true, Ordering::SeqCst);
    shared.kill.send_replace(true);
    shared.stdin.lock().await.take();
    // No answer can come now: dropping the senders tells anyone still waiting
    // (the handshake) at once, and no prompt is pending any more.
    lock(&shared.awaiting).clear();
    lock(&shared.permissions).clear();
    shared.sink.process_exited(exit);
    let _ = exited.send(true);
}

async fn drain_stderr(mut stderr: Box<dyn LineReader>, shared: Arc<Shared>) {
    while let Ok(Some(line)) = stderr.next_line().await {
        {
            let mut tail = lock(&shared.stderr_tail);
            if tail.len() == STDERR_TAIL {
                tail.pop_front();
            }
            tail.push_back(line.clone());
        }
        shared.sink.stderr_line(line);
    }
}

/// A prompt reserved for one `decide_permission` call. Dropping it releases
/// the prompt, on success and on every early return alike.
struct Reservation<'a> {
    deciding: &'a Mutex<HashSet<String>>,
    request_id: String,
}

impl<'a> Reservation<'a> {
    /// Reserves `request_id`, or `None` if another call holds it.
    fn take(deciding: &'a Mutex<HashSet<String>>, request_id: &str) -> Option<Self> {
        lock(deciding)
            .insert(request_id.to_string())
            .then(|| Reservation {
                deciding,
                request_id: request_id.to_string(),
            })
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        lock(self.deciding).remove(&self.request_id);
    }
}

/// A running CLI session.
struct ClaudeSession {
    shared: Arc<Shared>,
    /// Flips to true once the read loop has reported the exit.
    exited: watch::Receiver<bool>,
    stop_grace: Duration,
}

impl ClaudeSession {
    async fn kill_and_wait(&self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.shared.kill.send_replace(true);
        let mut exited = self.exited.clone();
        // Fails only if the read loop is gone, which also means it has ended.
        let _ = exited.wait_for(|exited| *exited).await;
    }

    /// The answer to a `can_use_tool` request (its raw `request` object), from
    /// the user's decision, as the `response` of the host's `control_response`.
    fn answer(request: &Value, decision: PermissionDecision) -> Result<Value, AdapterError> {
        match decision.behavior {
            PermissionBehavior::Allow => {
                // The user picked suggestions by their index in the event;
                // turn those back into the CLI's own objects, verbatim.
                let cli_suggestions = request["permission_suggestions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let offered = suggestions::offered(&cli_suggestions);
                let mut chosen = Vec::new();
                for index in &decision.apply_suggestions {
                    let (_, suggestion) = offered.get(*index as usize).ok_or_else(|| {
                        AdapterError::Protocol(format!(
                            "suggestion {index} is not one of the {} offered",
                            offered.len()
                        ))
                    })?;
                    chosen.push(suggestion.payload.clone());
                }
                // Always sent: the user's edit, or the input as requested.
                let input = decision
                    .updated_input
                    .map(Value::Object)
                    .unwrap_or_else(|| request["input"].clone());
                let mut answer = json!({ "behavior": "allow", "updatedInput": input });
                if !chosen.is_empty() {
                    answer["updatedPermissions"] = Value::Array(chosen);
                }
                Ok(answer)
            }
            PermissionBehavior::Deny => Ok(json!({
                "behavior": "deny",
                "message": decision
                    .message
                    .unwrap_or_else(|| "The user denied this tool call.".into()),
            })),
        }
    }
}

/// Dropping a session without stopping it kills the CLI rather than leaving
/// it running with nobody to answer it.
impl Drop for ClaudeSession {
    fn drop(&mut self) {
        self.shared.kill.send_replace(true);
    }
}

#[async_trait]
impl AdapterSession for ClaudeSession {
    async fn send_user(&self, text: &str) -> Result<(), AdapterError> {
        self.shared.write(&HostMessage::user_text(text)).await
    }

    async fn decide_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), AdapterError> {
        let unknown = || AdapterError::UnknownRequest(request_id.to_string());
        let request = lock(&self.shared.permissions)
            .get(request_id)
            .cloned()
            .ok_or_else(unknown)?;
        // Reserve the prompt: a second decision while this one is being
        // written is refused, as if the prompt were already answered.
        let _reserved = Reservation::take(&self.shared.deciding, request_id).ok_or_else(unknown)?;
        let message = HostMessage::ControlResponse(HostControlResponse {
            response: HostResponse::Success {
                request_id: request_id.to_string(),
                response: Self::answer(&request, decision)?,
            },
        });
        self.shared.write(&message).await?;
        // Only now, so a write that failed leaves the request answerable.
        // The reservation is released when `_reserved` drops, after this.
        lock(&self.shared.permissions).remove(request_id);
        Ok(())
    }

    async fn interrupt(&self) -> Result<(), AdapterError> {
        // The CLI's answer arrives on stdout and is recorded like any other
        // line; nothing here needs to wait for it.
        let request_id = self.shared.request_id("interrupt");
        let request = HostRequest::Interrupt {
            cancel_queued: None,
        };
        self.shared
            .write(&HostMessage::control_request(request_id, request))
            .await
    }

    async fn stop(&self) -> Result<(), AdapterError> {
        // No new lines from here on.
        self.shared.stopping.store(true, Ordering::SeqCst);
        let mut exited = self.exited.clone();
        let graceful = tokio::time::timeout(self.stop_grace, async {
            // Closing stdin is how the CLI is told there is no more input: it
            // finishes and exits 0 (fixtures 07 to 11). The lock is taken
            // inside the grace period: a write stuck on a CLI that stopped
            // reading holds it, and must not hold up the stop.
            self.shared.stdin.lock().await.take();
            let _ = exited.wait_for(|exited| *exited).await;
        });
        if graceful.await.is_err() {
            // Kill whatever the state of stdin; the stuck write gives up.
            self.kill_and_wait().await;
        }
        Ok(())
    }
}
