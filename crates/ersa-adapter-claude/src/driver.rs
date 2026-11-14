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

use ersa_adapter::recording::Recording;
use ersa_adapter::{
    Adapter, AdapterError, AdapterSession, AdapterSink, BackendChoices, ConversationEnd, Direction,
    ImageLimits, LineReader, LineWriter, OpenElsewhere, OutgoingMessage, PermissionDecision,
    Process, ProcessExit, ProcessSpec, Projector, QuestionReply, RecordDeletion, RecordEvent,
    RecordSummary, SessionConfig, TitleRules, Transport, async_trait,
};
use ersa_protocol::core_api::{Choice, ModelChoice};
use ersa_protocol::events::PermissionBehavior;
use serde_json::{Value, json};
use tokio::sync::{oneshot, watch};

use crate::projector::ClaudeProjector;
use crate::questions::{self, QUESTION_TOOL};
use crate::record::{self, LiveCheck};
use crate::suggestions;
use crate::wire::{
    HostControlResponse, HostMessage, HostRequest, HostResponse, InitializeRequest, RenameSource,
    WorkspaceTrust,
};

/// The message a declined question's deny carries: what the agent reads
/// before carrying on without answers (PROTO-CLAUDE-CODE-STREAM-R29).
const DECLINED_MESSAGE: &str = "The user declined to answer.";

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
    /// How long a host control request other than `initialize` waits for
    /// the CLI's answer.
    answer_timeout: Duration,
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
            // Well inside Core's own bound on an adapter call, so the CLI's
            // silence is what the user is told, not Core's timeout.
            answer_timeout: Duration::from_secs(10),
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

    /// How long a rename waits for the CLI's answer.
    pub fn with_answer_timeout(mut self, timeout: Duration) -> Self {
        self.answer_timeout = timeout;
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
    // The CLI's own `default` entry is its default: choosing it passes no
    // `--model` (PROTO-CLAUDE-CODE-STREAM, Invocation).
    if let Some(model) = config.model.as_ref().filter(|m| *m != DEFAULT_MODEL) {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(effort) = &config.effort {
        args.push("--effort".into());
        args.push(effort.clone());
    }
    args
}

/// The value of the model entry that is the CLI's own default, which
/// `initialize` lists first, "Default (recommended)".
const DEFAULT_MODEL: &str = "default";

/// The efforts and permission modes `backend.list` offers for Claude Code
/// (PROTO-CLAUDE-CODE-STREAM, Invocation): what the adapter knows the CLI
/// accepts, since the CLI reports no list of either. The CLI marks no
/// default effort, so none is marked here: a session given none runs at the
/// CLI's own. `default` asks before every action outside the allow rules
/// (FLOW-NEW-SESSION-R02). `bypassPermissions` is not offered. The models
/// are the CLI's own report ([`read_models`]).
fn choices() -> BackendChoices {
    let list = |items: &[(&str, &str)], default: Option<&str>| -> Vec<Choice> {
        items
            .iter()
            .map(|&(id, label)| Choice {
                id: id.into(),
                label: label.into(),
                default: Some(id) == default,
            })
            .collect()
    };
    BackendChoices {
        efforts: list(
            &[
                ("low", "Low"),
                ("medium", "Medium"),
                ("high", "High"),
                ("xhigh", "Extra high"),
                ("max", "Max"),
            ],
            None,
        ),
        permission_modes: list(
            &[
                ("default", "Ask before acting"),
                ("acceptEdits", "Accept edits"),
                ("plan", "Plan"),
                ("auto", "Auto"),
            ],
            Some("default"),
        ),
    }
}

/// The models in the CLI's `initialize` response (its `response` object),
/// in the CLI's order (PROTO-CLAUDE-CODE-STREAM-R39): each entry's `value`
/// is the id, `displayName` the label, `resolvedModel` the id a session of
/// it reports, and `supportedEffortLevels` its efforts, absent when the
/// entry has none. The entry whose value is `default` is the default. A
/// response without a list, or an entry without a value, is not one the
/// adapter can read.
pub fn read_models(response: &Value) -> Result<Vec<ModelChoice>, AdapterError> {
    let entries = response["models"].as_array().ok_or_else(|| {
        AdapterError::Protocol("the CLI's initialize response lists no models".into())
    })?;
    entries
        .iter()
        .map(|entry| {
            let id = entry["value"]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    AdapterError::Protocol(format!("a model entry without a value: {entry}"))
                })?;
            let text = |field: &str| entry[field].as_str().filter(|v| !v.is_empty());
            let efforts: Option<Vec<String>> = entry["supportedEffortLevels"]
                .as_array()
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect::<Vec<_>>()
                })
                .filter(|levels| !levels.is_empty());
            Ok(ModelChoice {
                id: id.to_string(),
                label: text("displayName").unwrap_or(id).to_string(),
                default: id == DEFAULT_MODEL,
                resolved: text("resolvedModel").map(String::from),
                efforts,
            })
        })
        .collect()
}

/// A sink for the model probe's process, whose lines belong to no session:
/// stderr goes to the log, everything else nowhere.
struct ProbeSink;

impl AdapterSink for ProbeSink {
    fn record(&self, _dir: Direction, _line: String) {}

    fn stderr_line(&self, line: String) {
        tracing::debug!(%line, "Claude Code model probe stderr");
    }

    fn process_exited(&self, _exit: io::Result<ProcessExit>) {}
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
    configured.unwrap_or_else(host_program)
}

/// Which `claude` to run where no session names one, as for the version
/// check that detects the CLI: the `ERSA_CLAUDE` environment variable, else
/// whatever `claude` the host finds. An empty value counts as unset.
fn host_program() -> String {
    std::env::var("ERSA_CLAUDE")
        .ok()
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| "claude".into())
}

/// The arguments that continue an earlier session, after
/// [`invocation_args`]: `--resume` with [`SessionConfig::resume`], the
/// `session_id` from that session's `system/init`, which Ersa stores with the
/// session. The CLI keeps that id and appends to its session record
/// (PROTO-CLAUDE-CODE-STREAM-R14). None for a new session or a fork.
///
/// The CLI's session ids are UUIDs, and anything else is an error, so a value
/// that merely looks like a flag (from a file name in the record directory,
/// say) can never reach the command line as one.
pub fn resume_args(config: &SessionConfig) -> Result<Vec<String>, AdapterError> {
    let Some(id) = &config.resume else {
        return Ok(Vec::new());
    };
    if crate::record::is_session_id(id) {
        Ok(vec!["--resume".into(), id.clone()])
    } else {
        Err(AdapterError::Protocol(format!(
            "cannot resume {id:?}: not a Claude Code session id"
        )))
    }
}

/// The arguments that fork an earlier session, after [`invocation_args`]:
/// `--resume` with the original's id, and `--fork-session`, so the CLI copies
/// its conversation into a new record and leaves the original's as it was
/// (fixtures 2.1.316/13 to 19, PROTO-CLAUDE-CODE-STREAM-R38). None unless
/// `fork_of` is set. Like a resume id, it must be a session id.
pub fn fork_args(config: &SessionConfig) -> Result<Vec<String>, AdapterError> {
    let Some(fork) = &config.fork_of else {
        return Ok(Vec::new());
    };
    let id = &fork.original;
    if config.resume.is_some() {
        return Err(AdapterError::Protocol(
            "a session is either resumed or forked, not both".into(),
        ));
    }
    if !crate::record::is_session_id(id) {
        return Err(AdapterError::Protocol(format!(
            "cannot fork {id:?}: not a Claude Code session id"
        )));
    }
    let mut args = vec!["--resume".into(), id.clone(), "--fork-session".into()];
    // The CLI announces the name before it answers `initialize`
    // (PROTO-CLAUDE-CODE-STREAM-R45).
    if let Some(name) = &fork.name {
        args.extend(["--name".into(), name.clone()]);
    }
    Ok(args)
}

/// The arguments that start a new session under the id Core chose and
/// stored, after [`invocation_args`]: `--session-id` with
/// `backend_session_id`, so the CLI's `system/init` reports that id and its
/// record is filed under it (PROTO-CLAUDE-CODE-STREAM-R35). None when Core
/// chose no id. A fork takes one too, beside its `--resume` (fixture
/// 2.1.316/16). Never with a plain `--resume`, which continues a session
/// under its own id: a config asking for both is an error.
///
/// Like a resume id, the id must be a UUID, so it can never reach the
/// command line as a flag.
pub fn session_id_args(config: &SessionConfig) -> Result<Vec<String>, AdapterError> {
    let Some(id) = &config.backend_session_id else {
        return Ok(Vec::new());
    };
    if config.resume.is_some() {
        return Err(AdapterError::Protocol(
            "a resumed session keeps its own id; it cannot also start under a new one".into(),
        ));
    }
    if !crate::record::is_session_id(id) {
        return Err(AdapterError::Protocol(format!(
            "cannot start a session as {id:?}: not a Claude Code session id"
        )));
    }
    Ok(vec!["--session-id".into(), id.clone()])
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

    /// `claude --version`, through the same lookup a session's CLI is found
    /// by, so detection checks the CLI sessions will run. It prints
    /// `2.1.302 (Claude Code)`, read by the default rule.
    fn version_command(&self) -> ProcessSpec {
        ProcessSpec {
            program: host_program(),
            args: vec!["--version".into()],
            ..ProcessSpec::default()
        }
    }

    /// One line of at most 200 characters: what the CLI's own session list
    /// shows (PROTO-CLAUDE-CODE-STREAM, Listing).
    fn title_rules(&self) -> TitleRules {
        TitleRules {
            max_chars: record::MAX_TITLE_CHARS,
            one_line: true,
        }
    }

    /// A Claude Code recording notes the CLI's version as
    /// `claude_code_version` (scripts/record-claude.mjs); without it the
    /// recording is not one this adapter can say it made, since replaying it
    /// would report a made-up version.
    fn recording_version(&self, recording: &Recording) -> Result<String, String> {
        recording.meta["claude_code_version"]
            .as_str()
            .map(str::trim)
            .filter(|version| !version.is_empty())
            .map(String::from)
            .ok_or_else(|| {
                "its notes do not say which Claude Code version made it (`claude_code_version`)"
                    .to_string()
            })
    }

    /// The CLI takes PNG, JPEG, GIF and WebP images of up to 5 MB each
    /// (PROTO-CLAUDE-CODE-STREAM, User message).
    fn image_limits(&self) -> ImageLimits {
        ImageLimits {
            media_types: &["image/png", "image/jpeg", "image/gif", "image/webp"],
            max_bytes: 5 * 1024 * 1024,
        }
    }

    fn choices(&self) -> BackendChoices {
        choices()
    }

    fn set_environment(&self, spec: &mut ProcessSpec) {
        spec.env_remove
            .extend(INHERITED_VARIABLES.map(String::from));
        spec.env.extend(entrypoint_env());
    }

    /// A fresh UUID, the form the CLI's own session ids take: the CLI starts
    /// the session under it (`--session-id`), so Core knows the session's id,
    /// and can rename it, before its first message
    /// (PROTO-CLAUDE-CODE-STREAM-R35).
    fn new_session_id(&self) -> Option<String> {
        Some(uuid::Uuid::new_v4().to_string())
    }

    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError> {
        let (session, _answer) = self.launch(transport, config, sink).await?;
        Ok(Box::new(session))
    }

    /// Starts the CLI on the host as a new session of no project, sends
    /// `initialize` and no message, reads the models from the answer, and
    /// closes stdin (PROTO-CLAUDE-CODE-STREAM-R39). It starts under an id of
    /// its own, as every new session does: a CLI started that way and given
    /// no message writes no session record (`initialize-only-2026-11-09.txt`).
    /// The time limit is the session handshake's.
    async fn models(
        &self,
        transport: Arc<dyn Transport>,
    ) -> Result<Vec<ModelChoice>, AdapterError> {
        let config = SessionConfig {
            permission_mode: "default".into(),
            backend_session_id: self.new_session_id(),
            ..SessionConfig::default()
        };
        let (session, answer) = self.launch(transport, config, Arc::new(ProbeSink)).await?;
        let models = read_models(&answer["response"]);
        // Closes stdin, and kills the CLI should it linger.
        let _ = session.stop().await;
        models
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

    /// How the record's conversation ends, read from this machine's files.
    async fn conversation_end(
        &self,
        _transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<Option<ConversationEnd>, AdapterError> {
        let home = self.claude_home()?;
        let (cwd, id) = (cwd.to_string(), backend_session_id.to_string());
        blocking(move || record::conversation_end(&home, &cwd, &id).map(Some)).await
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

    /// Appends one `custom-title` entry to the session's record on this
    /// machine (PROTO-CLAUDE-CODE-STREAM-R22). The record cache needs no
    /// telling: the file's new size is a new key.
    async fn rename_record(
        &self,
        _transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
        title: &str,
    ) -> Result<(), AdapterError> {
        let home = self.claude_home()?;
        let (cwd, id, title) = (
            cwd.to_string(),
            backend_session_id.to_string(),
            title.to_string(),
        );
        blocking(move || record::append_title(&home, &cwd, &id, &title)).await
    }

    /// Reads the live-session registry on this machine
    /// (PROTO-CLAUDE-CODE-STREAM-R17): like the record, a remote host's is
    /// not read yet (M4). With no registry at all no CLI has run, so nothing
    /// is open; one that cannot be read, or no home to find it in, makes the
    /// answer unknown, never "not open".
    async fn open_elsewhere(
        &self,
        _transport: Arc<dyn Transport>,
        backend_session_id: &str,
    ) -> OpenElsewhere {
        let Ok(home) = self.claude_home() else {
            return OpenElsewhere::Unknown;
        };
        let id = backend_session_id.to_string();
        let check = tokio::task::spawn_blocking(move || record::live_session(&home, &id)).await;
        match check {
            Ok(LiveCheck::NotOpen) => OpenElsewhere::No,
            Ok(LiveCheck::Open(open)) => OpenElsewhere::Yes {
                pids: open.iter().map(|live| live.pid).collect(),
            },
            Ok(LiveCheck::Unknown { .. }) | Err(_) => OpenElsewhere::Unknown,
        }
    }

    /// Deletes the session's record file and the folder of the same name
    /// beside it, on this machine (PROTO-CLAUDE-CODE-STREAM, Deleting), only
    /// when the live-session registry shows no process has it open
    /// (PROTO-CLAUDE-CODE-STREAM-R37). Core stops its own process for the
    /// session first, and a CLI removes its registry file as it exits, so any
    /// process still listed is another one. The registry is read and the
    /// files deleted on one thread, one straight after the other.
    async fn delete_record(
        &self,
        _transport: Arc<dyn Transport>,
        cwd: &str,
        backend_session_id: &str,
    ) -> Result<RecordDeletion, AdapterError> {
        let home = self.claude_home()?;
        let (cwd, id) = (cwd.to_string(), backend_session_id.to_string());
        let deletion = blocking(move || {
            match record::live_session(&home, &id) {
                LiveCheck::NotOpen => {}
                LiveCheck::Open(open) => {
                    return Ok(RecordDeletion::Refused(OpenElsewhere::Yes {
                        pids: open.iter().map(|live| live.pid).collect(),
                    }));
                }
                LiveCheck::Unknown { .. } => {
                    return Ok(RecordDeletion::Refused(OpenElsewhere::Unknown));
                }
            }
            record::delete_record(&home, &cwd, &id)?;
            Ok(RecordDeletion::Deleted)
        })
        .await?;
        if deletion == RecordDeletion::Deleted {
            self.records.forget(backend_session_id);
        }
        Ok(deletion)
    }
}

impl ClaudeAdapter {
    /// Starts the CLI on `transport` and performs the `initialize` handshake
    /// (PROTO-CLAUDE-CODE-STREAM-R02), returning the running session and the
    /// CLI's answer (the `control_response`'s `response` object).
    async fn launch(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<(ClaudeSession, Value), AdapterError> {
        // A bad resume or session id fails here, before anything starts.
        let resume = resume_args(&config)?;
        let fork = fork_args(&config)?;
        let session_id = session_id_args(&config)?;

        let mut spec = ProcessSpec {
            program: program(&config),
            args: invocation_args(&config),
            cwd: (!config.cwd.is_empty()).then(|| config.cwd.clone()),
            ..ProcessSpec::default()
        };
        self.set_environment(&mut spec);
        spec.args.extend(resume);
        spec.args.extend(fork);
        spec.args.extend(session_id);
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
            answer_timeout: self.answer_timeout,
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
                    Ok(answer)
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
            Ok(answer) => Ok((session, answer)),
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
    /// How long a host control request waits for the CLI's answer.
    answer_timeout: Duration,
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

impl ClaudeSession {
    /// Answers the pending `can_use_tool` request `request_id` with the
    /// `response` `answer` builds from its raw `request` object. Only a
    /// question when `question` is true, and only a permission request when
    /// it is false; any other id is refused as unknown.
    async fn respond(
        &self,
        request_id: &str,
        question: bool,
        answer: impl FnOnce(&Value) -> Result<Value, AdapterError>,
    ) -> Result<(), AdapterError> {
        let unknown = || AdapterError::UnknownRequest(request_id.to_string());
        let request = lock(&self.shared.permissions)
            .get(request_id)
            .cloned()
            .filter(|request| (request["tool_name"] == QUESTION_TOOL) == question)
            .ok_or_else(unknown)?;
        // Reserve the prompt: a second answer while this one is being
        // written is refused, as if the prompt were already answered.
        let _reserved = Reservation::take(&self.shared.deciding, request_id).ok_or_else(unknown)?;
        let message = HostMessage::ControlResponse(HostControlResponse {
            response: HostResponse::Success {
                request_id: request_id.to_string(),
                response: answer(&request)?,
            },
        });
        self.shared.write(&message).await?;
        // Only now, so a write that failed leaves the request answerable.
        // The reservation is released when `_reserved` drops, after this.
        lock(&self.shared.permissions).remove(request_id);
        Ok(())
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

    /// A message with attachments: its text, then its images, then the list
    /// of its files and folders (PROTO-CLAUDE-CODE-STREAM-R25). The line is
    /// recorded before it is written, so a write that fails reads as delivery
    /// unknown.
    async fn send_message(&self, message: &OutgoingMessage) -> Result<(), AdapterError> {
        // Refused before anything is recorded or written, so it reads as not sent.
        if let Some(reason) = crate::attachments::unsendable(message) {
            return Err(AdapterError::Protocol(reason));
        }
        self.shared
            .write(&HostMessage::user(crate::attachments::blocks(message)))
            .await
    }

    async fn decide_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), AdapterError> {
        // A question is answered with `answer_question`, never decided.
        self.respond(request_id, false, |request| Self::answer(request, decision))
            .await
    }

    /// An allow whose `updatedInput` is the request's input plus `answers`,
    /// keyed by question text, or a deny when the user declined
    /// (PROTO-CLAUDE-CODE-STREAM-R29).
    async fn answer_question(
        &self,
        request_id: &str,
        reply: QuestionReply,
    ) -> Result<(), AdapterError> {
        self.respond(request_id, true, |request| match reply {
            QuestionReply::Declined => Ok(json!({
                "behavior": "deny",
                "message": DECLINED_MESSAGE,
            })),
            QuestionReply::Answers(answers) => {
                let mut input = request["input"].as_object().cloned().unwrap_or_default();
                let (asked, _) = questions::asked(&input);
                let answers =
                    questions::to_cli(&asked, &answers).map_err(AdapterError::Protocol)?;
                input.insert("answers".into(), Value::Object(answers));
                Ok(json!({ "behavior": "allow", "updatedInput": input }))
            }
        })
        .await
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

    /// Sends `rename_session` and waits for the CLI's answer. The CLI then
    /// writes the `custom-title` entry itself, so Ersa writes nothing to the
    /// record (PROTO-CLAUDE-CODE-STREAM-R22). `source: host` makes it count
    /// as the user's own rename, and `session_id` makes a CLI that has moved
    /// to another session refuse rather than name that one.
    async fn rename(&self, backend_session_id: &str, title: &str) -> Result<(), AdapterError> {
        let request_id = self.shared.request_id("rename");
        // Registered before the write, so the read loop finds the waiter
        // however soon the answer comes. An exit clears every waiter, which
        // ends this wait too.
        let (answer_tx, answer_rx) = oneshot::channel();
        lock(&self.shared.awaiting).insert(request_id.clone(), answer_tx);
        let request = HostRequest::RenameSession {
            title: title.to_string(),
            source: Some(RenameSource::Host),
            session_id: Some(backend_session_id.to_string()),
        };
        let written = self
            .shared
            .write(&HostMessage::control_request(&request_id, request))
            .await;
        if let Err(e) = written {
            lock(&self.shared.awaiting).remove(&request_id);
            return Err(e);
        }
        // A CLI that never answers (one too old to know the request, or
        // wedged) must not keep the waiter for the rest of the session.
        let answer = tokio::time::timeout(self.answer_timeout, answer_rx).await;
        match answer {
            Ok(Ok(answer)) if answer["subtype"] == "success" => Ok(()),
            Ok(Ok(answer)) => Err(AdapterError::Protocol(format!(
                "the CLI refused the rename: {}",
                answer["error"].as_str().unwrap_or("no reason given")
            ))),
            // The read loop dropped the waiter: the CLI exited first.
            Ok(Err(_)) => Err(AdapterError::NotRunning),
            Err(_) => {
                lock(&self.shared.awaiting).remove(&request_id);
                Err(AdapterError::Protocol(format!(
                    "the CLI did not answer the rename within {:?}; it may still take it",
                    self.answer_timeout
                )))
            }
        }
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
