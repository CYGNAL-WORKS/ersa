//! Starting and driving one Claude Code CLI process per session.
//!
//! One task owns the process: it reads stdout line by line, hands each line to
//! the sink, routes control responses to whoever is waiting for them, and
//! finally reports the exit. The session methods write to stdin from whatever
//! task Core calls them on, through one lock, recording each line before it is
//! written (PROTO-CLAUDE-CODE-STREAM-R07; the transport flushes every line).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, Direction, LineReader, LineWriter,
    PermissionDecision, Process, ProcessSpec, Projector, SessionConfig, Transport, async_trait,
};
use ersa_protocol::events::PermissionBehavior;
use serde_json::Value;
use tokio::sync::{oneshot, watch};

use crate::projector::ClaudeProjector;
use crate::suggestions;
use crate::wire::{
    self, CanUseTool, CliMessage, CliRequest, CliResponse, HostControlResponse, HostMessage,
    HostRequest, HostResponse, InitializeRequest,
};

/// The Claude Code adapter (backend `claude-code`).
#[derive(Debug, Clone)]
pub struct ClaudeAdapter {
    handshake_timeout: Duration,
    stop_grace: Duration,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        ClaudeAdapter {
            // A cold CLI start took about four seconds in the recordings;
            // this leaves plenty of room for a slow disk or a remote host.
            handshake_timeout: Duration::from_secs(60),
            stop_grace: Duration::from_secs(3),
        }
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
        // An empty list: read no user, project, or local settings files, so
        // the user's own Claude Code configuration cannot change a session.
        "--setting-sources",
        "",
        "--strict-mcp-config",
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
    if let Some(model) = &config.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    args
}

/// Which `claude` to run: `extra.claude_path` from the session config, else the
/// `ERSA_CLAUDE` environment variable, else whatever `claude` the host finds.
fn program(config: &SessionConfig) -> String {
    let configured = config.extra.get("claude_path").and_then(Value::as_str);
    let from_env = std::env::var("ERSA_CLAUDE").ok();
    configured
        .map(String::from)
        .or(from_env)
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "claude".into())
}

#[async_trait]
impl Adapter for ClaudeAdapter {
    fn backend(&self) -> &str {
        crate::BACKEND
    }

    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        let spec = ProcessSpec {
            program: program(&config),
            args: invocation_args(&config),
            cwd: (!config.cwd.is_empty()).then(|| config.cwd.clone()),
            env: BTreeMap::new(),
        };
        let mut process = transport.spawn(spec).await?;
        let stdin = process.take_stdin().ok_or(AdapterError::NotRunning)?;
        let stdout = process.take_stdout().ok_or(AdapterError::NotRunning)?;
        if let Some(stderr) = process.take_stderr() {
            tokio::spawn(drain_stderr(stderr, sink.clone()));
        }

        let (exited_tx, exited_rx) = watch::channel(false);
        let (kill_tx, kill_rx) = watch::channel(false);
        let shared = Arc::new(Shared {
            sink,
            stdin: tokio::sync::Mutex::new(Some(stdin)),
            awaiting: Mutex::default(),
            permissions: Mutex::default(),
            next_id: AtomicU64::new(1),
        });
        let session = ClaudeSession {
            shared: shared.clone(),
            exited: exited_rx,
            kill: kill_tx,
            stop_grace: self.stop_grace,
        };

        // Register for the answer before the request can be written, so the
        // read loop always finds someone waiting for it.
        let init_id = shared.request_id("init");
        let (answer_tx, answer_rx) = oneshot::channel();
        lock(&shared.awaiting).insert(init_id.clone(), answer_tx);
        tokio::spawn(read_loop(
            shared.clone(),
            stdout,
            process,
            kill_rx,
            exited_tx,
        ));

        // The handshake (PROTO-CLAUDE-CODE-STREAM-R02): no user message goes
        // out until the CLI has answered `initialize`, and `start` returns only
        // then, so the answer is in the raw log before anything Core sends.
        let request = HostRequest::Initialize(InitializeRequest::default());
        let handshake = async {
            shared
                .write(&HostMessage::control_request(&init_id, request))
                .await?;
            match tokio::time::timeout(self.handshake_timeout, answer_rx).await {
                Ok(Ok(CliResponse::Success(_))) => Ok(()),
                Ok(Ok(CliResponse::Error(e))) => Err(AdapterError::Protocol(format!(
                    "the CLI refused initialize: {}",
                    e.error
                ))),
                Ok(Ok(CliResponse::Other(other))) => Err(AdapterError::Protocol(format!(
                    "unexpected answer to initialize: {other}"
                ))),
                // The read loop dropped the sender: stdout ended first.
                Ok(Err(_)) => Err(AdapterError::Protocol(
                    "the CLI exited before answering initialize".into(),
                )),
                Err(_) => Err(AdapterError::Protocol(format!(
                    "the CLI did not answer initialize within {:?}",
                    self.handshake_timeout
                ))),
            }
        };
        match handshake.await {
            Ok(()) => Ok(Box::new(session)),
            Err(e) => {
                // A half-started CLI is of no use; end it before reporting.
                session.kill_and_wait().await;
                Err(e)
            }
        }
    }

    fn projector(&self) -> Box<dyn Projector> {
        Box::new(ClaudeProjector::new())
    }
}

/// What the read loop and the session methods share.
struct Shared {
    sink: Arc<dyn AdapterSink>,
    /// `None` once stdin is closed, by `stop` or because the process exited.
    stdin: tokio::sync::Mutex<Option<Box<dyn LineWriter>>>,
    /// Host control requests waiting for the CLI's answer, by `request_id`.
    awaiting: Mutex<HashMap<String, oneshot::Sender<CliResponse>>>,
    /// `can_use_tool` requests the CLI is waiting on, by `request_id`.
    permissions: Mutex<HashMap<String, CanUseTool>>,
    next_id: AtomicU64,
}

impl Shared {
    /// A fresh `request_id` for a host control request, such as `ersa-init-1`.
    fn request_id(&self, what: &str) -> String {
        let n = self.next_id.fetch_add(1, Ordering::Relaxed);
        format!("ersa-{what}-{n}")
    }

    /// Records `message` as a host line, then writes it.
    ///
    /// The stdin lock is held across both, so lines are recorded in the order
    /// they are written even when several tasks write at once.
    async fn write(&self, message: &HostMessage) -> Result<(), AdapterError> {
        let line = message
            .to_line()
            .map_err(|e| AdapterError::Protocol(format!("could not serialize a host line: {e}")))?;
        let mut stdin = self.stdin.lock().await;
        let writer = stdin.as_mut().ok_or(AdapterError::NotRunning)?;
        self.sink.record(Direction::Host, line.clone());
        writer.write_line(&line).await?;
        Ok(())
    }

    /// Handles one stdout line: records it, and keeps the driver's view of
    /// pending requests in step with it.
    async fn on_backend_line(&self, line: String) {
        // A line that does not parse is still recorded; the projector reports it.
        let message = CliMessage::from_line(&line).ok();

        // Pending permission requests change before the line is recorded, so a
        // decision Core makes as soon as it sees the line finds the request.
        let mut unsupported = None;
        match &message {
            Some(CliMessage::ControlRequest(request)) => match &request.request {
                CliRequest::CanUseTool(ask) => {
                    lock(&self.permissions).insert(request.request_id.clone(), (**ask).clone());
                }
                CliRequest::Other(body) => {
                    let subtype = body.get("subtype").and_then(Value::as_str).unwrap_or("");
                    unsupported = Some((request.request_id.clone(), subtype.to_string()));
                }
            },
            // The CLI withdrew a prompt (after an interrupt, say); a late
            // decision for it is refused rather than written.
            Some(CliMessage::ControlCancelRequest(cancel)) => {
                lock(&self.permissions).remove(&cancel.request_id);
            }
            _ => {}
        }

        self.sink.record(Direction::Backend, line);

        // Answers are delivered after recording, so whoever waits on one (the
        // handshake in `start`) resumes with the answer already in the log.
        if let Some(CliMessage::ControlResponse(answer)) = message {
            let waiting =
                response_id(&answer.response).and_then(|id| lock(&self.awaiting).remove(&id));
            if let Some(waiting) = waiting {
                // The waiter may have given up (timed out); nothing to do then.
                let _ = waiting.send(answer.response);
            }
        }

        // The CLI waits for an answer to every control request it sends, and
        // an unanswered one stalls the turn. Ersa only declares what it
        // handles, so this should not happen; if it does, say no rather than
        // leave the CLI hanging.
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

/// The `request_id` a control response answers.
fn response_id(response: &CliResponse) -> Option<String> {
    match response {
        CliResponse::Success(s) => Some(s.request_id.clone()),
        CliResponse::Error(e) => Some(e.request_id.clone()),
        CliResponse::Other(v) => v
            .get("request_id")
            .and_then(Value::as_str)
            .map(String::from),
    }
}

/// Locks a std mutex, carrying on if a panicking thread poisoned it: every map
/// here stays consistent between single insertions and removals.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The task that owns the process: reads stdout until it ends, then reports
/// the exit, exactly once and after the last record.
async fn read_loop(
    shared: Arc<Shared>,
    mut stdout: Box<dyn LineReader>,
    mut process: Box<dyn Process>,
    mut kill: watch::Receiver<bool>,
    exited: watch::Sender<bool>,
) {
    // What woke the loop. `select!` only picks; the work happens after it,
    // once the losing branch's future (which holds a lock on the kill flag
    // while polled) is gone, so nothing un-`Send` lives across an await.
    enum Next {
        Line(std::io::Result<Option<String>>),
        Kill,
    }

    let mut killed = false;
    loop {
        let next = tokio::select! {
            line = stdout.next_line() => Next::Line(line),
            _ = kill.wait_for(|kill| *kill), if !killed => Next::Kill,
        };
        match next {
            Next::Line(Ok(Some(line))) => shared.on_backend_line(line).await,
            // End of stdout, or a broken pipe: either way nothing more will be
            // read, and the exit status says what happened.
            Next::Line(Ok(None) | Err(_)) => break,
            // `stop` ran out of patience, or the session was dropped. Keep
            // reading afterwards: lines the CLI wrote before it died are still
            // in the pipe, and every line belongs in the log.
            Next::Kill => {
                let _ = process.kill().await;
                killed = true;
            }
        }
    }

    // Stdout can end before the process does; a kill request still applies.
    let exit = if killed {
        process.wait().await
    } else {
        let killed_while_waiting = tokio::select! {
            exit = process.wait() => Err(exit),
            _ = kill.wait_for(|kill| *kill) => Ok(()),
        };
        match killed_while_waiting {
            Err(exit) => exit,
            Ok(()) => {
                let _ = process.kill().await;
                process.wait().await
            }
        }
    };

    // Close stdin under its lock: any write still in progress finishes (and is
    // recorded) first, and none can be recorded after the exit is reported.
    shared.stdin.lock().await.take();
    // No answer can come now: dropping the senders tells anyone still waiting
    // (the handshake) at once, and no prompt is pending any more.
    lock(&shared.awaiting).clear();
    lock(&shared.permissions).clear();
    shared.sink.process_exited(exit);
    let _ = exited.send(true);
}

async fn drain_stderr(mut stderr: Box<dyn LineReader>, sink: Arc<dyn AdapterSink>) {
    while let Ok(Some(line)) = stderr.next_line().await {
        sink.stderr_line(line);
    }
}

/// A running CLI session.
struct ClaudeSession {
    shared: Arc<Shared>,
    /// Flips to true once the read loop has reported the exit.
    exited: watch::Receiver<bool>,
    /// Asks the read loop to kill the process.
    kill: watch::Sender<bool>,
    stop_grace: Duration,
}

impl ClaudeSession {
    async fn kill_and_wait(&self) {
        self.kill.send_replace(true);
        let mut exited = self.exited.clone();
        // Fails only if the read loop is gone, which also means it has ended.
        let _ = exited.wait_for(|exited| *exited).await;
    }

    /// The answer to a `can_use_tool` request, from the user's decision.
    fn answer(
        request: &CanUseTool,
        decision: PermissionDecision,
    ) -> Result<wire::PermissionDecision, AdapterError> {
        match decision.behavior {
            PermissionBehavior::Allow => {
                // The user picked suggestions by their index in the event;
                // turn those back into the CLI's own objects.
                let offered = suggestions::offered(&request.permission_suggestions);
                let mut chosen = Vec::new();
                for index in &decision.apply_suggestions {
                    let (cli_index, _) = offered.get(*index as usize).ok_or_else(|| {
                        AdapterError::Protocol(format!(
                            "suggestion {index} is not one of the {} offered",
                            offered.len()
                        ))
                    })?;
                    chosen.push(request.permission_suggestions[*cli_index].clone());
                }
                Ok(wire::PermissionDecision::Allow {
                    // Always sent: the user's edit, or the input as requested.
                    updated_input: decision
                        .updated_input
                        .map(Value::Object)
                        .unwrap_or_else(|| request.input.clone()),
                    updated_permissions: (!chosen.is_empty()).then_some(chosen),
                })
            }
            PermissionBehavior::Deny => Ok(wire::PermissionDecision::Deny {
                message: decision
                    .message
                    .unwrap_or_else(|| "The user denied this tool call.".into()),
                interrupt: None,
            }),
        }
    }
}

/// Dropping a session without stopping it kills the CLI rather than leaving
/// it running with nobody to answer it.
impl Drop for ClaudeSession {
    fn drop(&mut self) {
        self.kill.send_replace(true);
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
        // Build the answer before taking the request off the pending list, so
        // a decision that cannot be built leaves the request answerable.
        let answer = Self::answer(&request, decision)?;
        if lock(&self.shared.permissions).remove(request_id).is_none() {
            // Withdrawn by the CLI, or answered by another call, meanwhile.
            return Err(unknown());
        }
        let message = HostMessage::permission_decision(request_id, &answer)
            .map_err(|e| AdapterError::Protocol(format!("could not serialize a decision: {e}")))?;
        self.shared.write(&message).await
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
        // Closing stdin is how the CLI is told there is no more input: it
        // finishes and exits 0 (fixtures 07 to 11).
        self.shared.stdin.lock().await.take();
        let mut exited = self.exited.clone();
        let graceful = tokio::time::timeout(self.stop_grace, exited.wait_for(|exited| *exited));
        if graceful.await.is_err() {
            self.kill_and_wait().await;
        }
        Ok(())
    }
}
