//! The traits an adapter implements, and the one Core implements for it.

use std::fmt;
use std::io;
use std::sync::Arc;

use async_trait::async_trait;
use ersa_protocol::core_api::{Choice, ModelChoice};
use ersa_protocol::events::{Answer, Event, PermissionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use time::OffsetDateTime;

use crate::raw::{Direction, RawLine};
use crate::recording::Recording;
use crate::transport::{ProcessExit, ProcessSpec, Transport};

/// One backend's adapter. Core holds one of each in its adapter registry.
#[async_trait]
pub trait Adapter: Send + Sync {
    /// The backend's name, as used in `source.backend` and the Core API, such
    /// as `"claude-code"`.
    fn backend(&self) -> &str;

    /// The backend's own display name, as `backend.list` reports it, such
    /// as `"Claude Code"`. The default is [`Adapter::backend`].
    fn label(&self) -> &str {
        self.backend()
    }

    /// Starts a backend session on `transport` and returns once the backend is
    /// ready for its first user message.
    ///
    /// From the moment the process starts, every line the adapter reads from
    /// the backend's stdout or writes to its stdin goes to `sink`, in order,
    /// including any handshake the adapter performs before returning. When the
    /// process exits, the adapter reports it to `sink` exactly once.
    async fn start(
        &self,
        transport: Arc<dyn Transport>,
        config: SessionConfig,
        sink: Arc<dyn AdapterSink>,
    ) -> Result<Box<dyn AdapterSession>, AdapterError>;

    /// Sets the environment every process of this backend runs with: its
    /// sessions, and the version check Core runs to detect it, so both see
    /// the backend configured alike. The default leaves the inherited
    /// environment as it is.
    fn set_environment(&self, _spec: &mut ProcessSpec) {}

    /// The command Core runs to detect the backend on a host: its version
    /// command, which [`Adapter::read_version`] reads. Core adds the
    /// adapter's environment ([`Adapter::set_environment`]). The default:
    /// `<backend> --version`.
    fn version_command(&self) -> ProcessSpec {
        ProcessSpec {
            program: self.backend().to_string(),
            args: vec!["--version".to_string()],
            ..ProcessSpec::default()
        }
    }

    /// The backend's version in what its version command printed to
    /// stdout, one entry per line; `None` when it printed nothing usable.
    /// The default: [`first_word_version`] of the first line.
    fn read_version(&self, stdout: &[String]) -> Option<String> {
        stdout.first().and_then(|line| first_word_version(line))
    }

    /// The titles the backend takes for a session, which Core applies to
    /// `session.set_title` (PROTO-CORE-API-R70). No default: every backend
    /// has its own.
    fn title_rules(&self) -> TitleRules;

    /// The backend version that made `recording`, read from its notes, when
    /// the recording is one of this adapter's backend; otherwise why it is
    /// not, which the replay server gives when no registered adapter
    /// recognizes a recording (PROTO-CORE-API-R69). The default: no
    /// recording is.
    fn recording_version(&self, _recording: &Recording) -> Result<String, String> {
        Err(format!("{} has no recordings", self.label()))
    }

    /// The oldest and newest backend versions this adapter was tested with,
    /// as `backend.list` reports them. `None`, the default, when it does not
    /// say.
    fn tested_versions(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    /// The efforts and permission modes a session of this backend can start
    /// with, as `backend.list` reports them: the adapter's knowledge of its
    /// tested versions, for which the backend reports no list. None, the
    /// default, for a backend that offers no choice.
    fn choices(&self) -> BackendChoices {
        BackendChoices::default()
    }

    /// The models a session of this backend can start with, as the backend
    /// itself reports them on the host behind `transport`
    /// (PROTO-CORE-API-R72), in the backend's order. Core asks only once
    /// the version check has found the backend. An `Io` error of kind
    /// `NotFound` means the backend is missing from the host; any other
    /// error, that it is present but could not report them. None, the
    /// default, for a backend that offers no choice of model.
    async fn models(
        &self,
        _transport: Arc<dyn Transport>,
    ) -> Result<Vec<ModelChoice>, AdapterError> {
        Ok(Vec::new())
    }

    /// The images this backend takes inline with a message: Core refuses any
    /// other before sending (PROTO-CORE-API, Turns and attachments). The
    /// default: none.
    fn image_limits(&self) -> ImageLimits {
        ImageLimits::default()
    }

    /// A fresh projector for one session's live stream. Core makes one each
    /// time it starts the backend and feeds it every raw line from the first.
    fn projector(&self) -> Box<dyn Projector>;

    /// An id for a new session, for a backend that starts a session under an
    /// id its host chooses: Core stores it as the session's backend session
    /// id before it starts the backend, and passes it in
    /// [`SessionConfig::backend_session_id`], so the session can be named,
    /// renamed and found from the moment it starts. `None`, the default, for
    /// a backend that names its sessions itself, announcing the id once it
    /// has started.
    fn new_session_id(&self) -> Option<String> {
        None
    }

    /// The events of a session's history, projected from the backend's own
    /// session record (ADR 0017): the session the backend knows as
    /// `backend_session_id`, started in `cwd` on the transport's host.
    ///
    /// A record that does not exist (never written yet, or deleted by the
    /// backend's retention) is an empty history, not an error. Pure in the
    /// record: reading the same record twice yields the same events
    /// (PROTO-AGENT-EVENTS-R11). A backend without a record has no history,
    /// which is the default. Shared rather than owned, so an adapter that
    /// keeps what it read hands it on without copying it.
    async fn read_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        _backend_session_id: &str,
    ) -> Result<Arc<Vec<RecordEvent>>, AdapterError> {
        Ok(Arc::default())
    }

    /// A token that changes whenever session `backend_session_id`'s record
    /// does (for a file: its size and modified time), so Core can tell
    /// whether events it read earlier still describe it. `None` when there
    /// is no record, or the adapter cannot tell, which is the default.
    async fn record_version(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        _backend_session_id: &str,
    ) -> Result<Option<String>, AdapterError> {
        Ok(None)
    }

    /// Lets go of anything kept about session `backend_session_id`'s
    /// record, which Ersa no longer shows (the session was removed).
    fn forget_record(&self, _cwd: &str, _backend_session_id: &str) {}

    /// What the backend's session record says of the session as a whole: its
    /// title and when it began. Read without reading the whole record, since
    /// Core asks for every session it lists. `None` when the session has no
    /// record, which is also the default.
    async fn summarize_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        _backend_session_id: &str,
    ) -> Result<Option<RecordSummary>, AdapterError> {
        Ok(None)
    }

    /// What session `backend_session_id`'s record says of its model, effort
    /// and permission mode, read for a relaunch: back from the record's end
    /// until each value is found or the record reaches the time Core last
    /// gave the backend that value. Core picks the values of the turns after
    /// its requests with [`RecordedSettings::since`]. A session with no
    /// record has none, which is also the default.
    async fn record_settings(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        _backend_session_id: &str,
        _requested: Requested,
    ) -> Result<RecordedSettings, AdapterError> {
        Ok(RecordedSettings::default())
    }

    /// Renames session `backend_session_id`, which no process of Ersa's has
    /// open, in the backend's own session record, so the backend's own tools
    /// show the new title too (PROTO-CORE-API-R12). A live session is renamed
    /// through [`AdapterSession::rename`] instead. A session with no record
    /// is an `Io` error of kind `NotFound`. The default: the backend cannot
    /// rename a session.
    async fn rename_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        _backend_session_id: &str,
        _title: &str,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::Protocol(format!(
            "the {} backend cannot rename a session",
            self.backend()
        )))
    }

    /// Whether a process other than Ersa's has session `backend_session_id`
    /// open on the transport's host, asked before Core resumes the session
    /// or writes to its record (PROTO-AGENT-EVENTS-R14), which it does
    /// whatever the answer, telling the user, and before it forks the
    /// session, which it refuses when the answer is yes. The default, for a
    /// backend that keeps no registry of open sessions: not open.
    async fn open_elsewhere(
        &self,
        _transport: Arc<dyn Transport>,
        _backend_session_id: &str,
    ) -> OpenElsewhere {
        OpenElsewhere::No
    }

    /// Deletes session `backend_session_id`'s record from the backend, for
    /// the user's Delete, so neither Ersa nor the backend's own tools list it
    /// again (ADR 0026). Refused, with nothing deleted, when the backend's
    /// registry shows another process has the session open or cannot say,
    /// since that process would go on writing the record. A record already
    /// gone counts as deleted. The default: the backend cannot delete a
    /// session.
    async fn delete_record(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        _backend_session_id: &str,
    ) -> Result<RecordDeletion, AdapterError> {
        Err(AdapterError::Protocol(format!(
            "the {} backend cannot delete a session",
            self.backend()
        )))
    }

    /// How session `backend_session_id`'s conversation ends, as its record
    /// holds it, which is what decides whether it can be forked
    /// (FLOW-FORK-SESSION-R03). A session with no record has
    /// [`ConversationEnd::NoReply`]. `None`, the default, for a backend
    /// that cannot fork a session.
    async fn conversation_end(
        &self,
        _transport: Arc<dyn Transport>,
        _cwd: &str,
        _backend_session_id: &str,
    ) -> Result<Option<ConversationEnd>, AdapterError> {
        Ok(None)
    }
}

/// How a session's conversation ends, read from its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationEnd {
    /// With a finished reply: the session can be forked.
    Finished,
    /// No reply has finished yet.
    NoReply,
    /// The last turn was interrupted, or ended in an error.
    Interrupted,
    /// Part way through a turn: one still being written, or one the process
    /// that was writing it never finished.
    Unfinished,
}

/// What [`Adapter::delete_record`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordDeletion {
    /// The record is gone, or was already.
    Deleted,
    /// Nothing was deleted: another process has the session open
    /// ([`OpenElsewhere::Yes`]) or the registry could not say
    /// ([`OpenElsewhere::Unknown`]).
    Refused(OpenElsewhere),
}

/// Whether another process has a session open, as the backend's own
/// registry of open sessions tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenElsewhere {
    /// No other process has it open.
    No,
    /// These processes have it open, in order of pid, so writing beside
    /// them would put two writers on one record. Never empty.
    Yes { pids: Vec<u32> },
    /// The registry could not be read well enough to tell.
    Unknown,
}

/// The efforts and permission modes a session of a backend can start with.
/// Each list marks the one a new session starts with when none is named, or
/// none when the backend marks none (Claude Code marks no effort). The
/// models are the backend's own report ([`Adapter::models`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackendChoices {
    pub efforts: Vec<Choice>,
    pub permission_modes: Vec<Choice>,
}

impl BackendChoices {
    /// The id the list marks as its default; empty when it marks none, as
    /// for a backend that offers no choice.
    pub fn default_of(list: &[Choice]) -> String {
        list.iter()
            .find(|c| c.default)
            .map(|c| c.id.clone())
            .unwrap_or_default()
    }
}

/// The version number in a version command's line: its first word, when
/// that starts with a digit. Claude Code prints `2.1.302 (Claude Code)`.
fn first_word_version(line: &str) -> Option<String> {
    let word = line.split_whitespace().next()?;
    word.starts_with(|c: char| c.is_ascii_digit())
        .then(|| word.to_string())
}

/// The titles a backend takes for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleRules {
    /// The most characters (not bytes) a title may have.
    pub max_chars: usize,
    /// Whether a title is a single line. When it is, Core turns line
    /// breaks into spaces; otherwise it keeps them.
    pub one_line: bool,
}

/// A session as its record presents it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordSummary {
    /// The session's title, as the backend's own tools show it; `None` when
    /// the record gives none.
    pub title: Option<String>,
    /// When the session began, RFC 3339, as the record gives it.
    pub created_at: Option<String>,
    /// The model, effort and permission mode the part of the record read
    /// for the summary holds, each with when its turn began; Core picks
    /// those of the turns after its own request with
    /// [`RecordedSettings::since`].
    pub settings: RecordedSettings,
}

/// What a session's record says of its model, effort and permission mode,
/// each from the latest turn that holds it (PROTO-CORE-API-R71). `None` for
/// a value the part of the record read does not hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordedSettings {
    pub model: Option<Recorded>,
    pub effort: Option<Recorded>,
    pub permission_mode: Option<Recorded>,
}

/// One value from a session's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The backend's own id for it, such as `claude-opus-5-5` or `plan`.
    pub value: String,
    /// When the turn it is from began, on the host's clock; `None` when the
    /// part of the record read does not say.
    pub turn_began: Option<OffsetDateTime>,
}

/// When Core last gave the backend each value, on the host's clock
/// (PROTO-CORE-API-R71); `None` for a value it never gave.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Requested {
    pub model: Option<OffsetDateTime>,
    pub effort: Option<OffsetDateTime>,
    pub permission_mode: Option<OffsetDateTime>,
}

impl RecordedSettings {
    /// Only the values from a turn that began after Core's request for
    /// each, or from any turn for a value Core never requested. A value
    /// whose turn's start is unknown counts only then.
    pub fn since(self, requested: Requested) -> RecordedSettings {
        let after = |recorded: Option<Recorded>, request: Option<OffsetDateTime>| {
            recorded.filter(|r| match request {
                None => true,
                Some(request) => r.turn_began.is_some_and(|began| began > request),
            })
        };
        RecordedSettings {
            model: after(self.model, requested.model),
            effort: after(self.effort, requested.effort),
            permission_mode: after(self.permission_mode, requested.permission_mode),
        }
    }
}

/// One event projected from an entry of a session record.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordEvent {
    /// The backend's id for the message the entry carries, on which record
    /// and live input are merged. `None` for an entry with no id (a line
    /// that could not be read, say).
    pub message_id: Option<String>,
    /// When the backend wrote the entry, as the entry gives it.
    pub ts: Option<String>,
    /// The backend version that wrote the entry, as the entry gives it.
    pub backend_version: Option<String>,
    pub event: Event,
}

/// The images a backend takes inline: their media types, and the most bytes
/// one may have.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageLimits {
    /// Such as `image/png`. Empty: the backend takes no images.
    pub media_types: &'static [&'static str],
    pub max_bytes: u64,
}

impl ImageLimits {
    /// Whether an image of this type and size is one the backend takes.
    pub fn allows(&self, media_type: &str, bytes: u64) -> bool {
        self.media_types.contains(&media_type) && bytes <= self.max_bytes
    }
}

/// A user message as Core hands it to an adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutgoingMessage {
    /// May be empty when there are attachments.
    pub text: String,
    /// In the order the user added them.
    pub attachments: Vec<OutgoingAttachment>,
}

/// One attachment as the adapter puts it into the backend's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutgoingAttachment {
    /// An image the agent sees, as base64 `data`.
    Image {
        media_type: String,
        data: String,
    },
    /// A file or folder, by its path relative to the session's workspace,
    /// with `/` between its parts.
    File {
        path: String,
    },
    Folder {
        path: String,
    },
}

/// A running backend session, driven by Core.
///
/// Methods take `&self` so Core can call them from any task without holding a
/// lock of its own; an adapter guards its stdin writer internally. Every line a
/// method writes to the backend goes to the sink first (see
/// [`AdapterSink::record`]).
#[async_trait]
pub trait AdapterSession: Send + Sync {
    /// Sends a user message to the backend.
    async fn send_user(&self, text: &str) -> Result<(), AdapterError>;

    /// Sends a user message with its attachments, which Core has already
    /// checked against [`Adapter::image_limits`] and, for files and folders,
    /// made paths relative to the workspace. The default sends a message
    /// without attachments as [`send_user`](Self::send_user) and refuses one
    /// with them, before writing anything, for a backend that has no way to
    /// carry them yet.
    async fn send_message(&self, message: &OutgoingMessage) -> Result<(), AdapterError> {
        if !message.attachments.is_empty() {
            return Err(AdapterError::Protocol(
                "this backend cannot take attachments yet".into(),
            ));
        }
        self.send_user(&message.text).await
    }

    /// Answers the backend's pending permission request `request_id`, the id
    /// carried by its `permission.requested` event.
    async fn decide_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), AdapterError>;

    /// Answers the backend's pending question `request_id`, the id carried
    /// by its `question.asked` event, with the user's answers or their
    /// declining it. The default: the backend asks no questions, so none can
    /// be pending.
    async fn answer_question(
        &self,
        request_id: &str,
        _reply: QuestionReply,
    ) -> Result<(), AdapterError> {
        Err(AdapterError::UnknownRequest(request_id.to_string()))
    }

    /// Asks the backend to stop the current turn. The session stays alive.
    async fn interrupt(&self) -> Result<(), AdapterError>;

    /// Renames the session, which the backend knows as `backend_session_id`,
    /// and returns once the backend has taken the new title. The default:
    /// the backend cannot rename a session.
    async fn rename(&self, _backend_session_id: &str, _title: &str) -> Result<(), AdapterError> {
        Err(AdapterError::Protocol(
            "this backend cannot rename a session".into(),
        ))
    }

    /// Ends the backend process, gracefully if the backend supports it and by
    /// killing it otherwise, and returns once it has exited.
    async fn stop(&self) -> Result<(), AdapterError>;
}

/// Where an adapter delivers what crosses the backend's stdio. Core implements
/// it; the adapter never sees the raw log or the event stream.
///
/// The methods are synchronous and must return quickly: Core stamps the line
/// and queues it for projection, never blocking on I/O. That lets an adapter call
/// them from inside its read loop without awaiting.
pub trait AdapterSink: Send + Sync {
    /// Records one raw line. Core assigns its `index` and `ts` and appends it to
    /// the session's raw log.
    ///
    /// Backend lines are recorded as they are read. Host lines are recorded
    /// immediately before they are written, never after: the backend can answer
    /// as soon as it reads a line, and recording first is what keeps the answer
    /// after the line that caused it in the log. If the write then fails, the
    /// session is broken anyway and the process exit says so.
    fn record(&self, dir: Direction, line: String);

    /// One line the backend wrote to stderr. Not part of the raw log: stderr is
    /// diagnostics, not protocol. Core logs it.
    fn stderr_line(&self, line: String);

    /// The backend process has ended: `Ok` with how it exited, or `Err` when
    /// the transport lost track of it. Called exactly once per session, after
    /// the last `record`.
    fn process_exited(&self, exit: io::Result<ProcessExit>);
}

/// Turns one live session's raw lines into Ersa events
/// (`PROTO-AGENT-EVENTS`, Projection rules).
///
/// A projector is a pure function of the lines it is fed, in order: no clock,
/// no I/O, no randomness, so the same log always gives the same events
/// (PROTO-AGENT-EVENTS-R03). It may keep state between lines (`&mut self`), such
/// as the text of a block being streamed or the turn number.
pub trait Projector: Send {
    /// The version of this adapter's projection logic. Bump it whenever the
    /// events this projector produces for some input would change. Nothing
    /// projected is stored, so the new events apply the next time a session
    /// is opened.
    fn version(&self) -> u32;

    /// The events `line` completes. Every line yields at least one, with
    /// `backend.passthrough` as the fallback (PROTO-AGENT-EVENTS-R02), and a
    /// line that cannot be projected yields a recoverable `error` plus a
    /// passthrough of the line, never a panic (PROTO-AGENT-EVENTS-R10).
    ///
    /// Payloads only: Core wraps each in an envelope, assigning `seq` and
    /// taking `ts` from `line`.
    fn project(&mut self, line: &RawLine) -> Vec<Projected>;

    /// The latest title the backend announced on the live stream for the
    /// session, from the lines projected so far: a name the user gave it,
    /// which its record may not hold yet, with how many announcements there
    /// have been, so that one repeating the last title still counts as one.
    /// Core shows it as the session's title, in memory only, as it does any
    /// detail only the live stream carries. `None`, the default, before any
    /// announcement, and for a backend that makes none.
    fn announced_title(&self) -> Option<(u64, &str)> {
        None
    }

    /// The session's model as the live stream last reported it, from the
    /// lines projected so far. Core lists it as the session's model while
    /// the session is live (PROTO-CORE-API-R71). `None`, the default, before
    /// any report, and for a backend that makes none.
    fn reported_model(&self) -> Option<&str> {
        None
    }

    /// The session's permission mode as the live stream last reported it,
    /// like [`Projector::reported_model`].
    fn reported_permission_mode(&self) -> Option<&str> {
        None
    }

    /// How many messages the backend holds behind its running turn, from
    /// the lines projected so far: messages handed over during a turn that
    /// the backend has not yet taken in. Core shows it as the tile's
    /// `waiting` while the session is live (PROTO-CORE-API-R81). 0, the
    /// default, for a backend that holds none.
    fn waiting(&self) -> u32 {
        0
    }
}

/// One event a projector produced, with the backend's id for the message it
/// came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Projected {
    /// The id the backend's session record gives the same message, when the
    /// line carried one. Core yields an event once when the record already
    /// had its message (PROTO-AGENT-EVENTS-R12), so set it only for events the
    /// record also yields.
    pub message_id: Option<String>,
    pub event: Event,
}

impl Projected {
    /// An event with no message in the session record behind it: a turn
    /// boundary, a permission prompt, usage, a state change.
    pub fn from_line(_line: &RawLine, event: Event) -> Self {
        Projected {
            message_id: None,
            event,
        }
    }

    /// An event from the message the backend identifies as `message_id`.
    pub fn of_message(message_id: Option<String>, event: Event) -> Self {
        Projected { message_id, event }
    }
}

/// What a fork starts from (FLOW-FORK-SESSION).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ForkOf {
    /// The backend's id for the session it starts from a copy of.
    pub original: String,
    /// The name the backend gives it as it starts, if any.
    #[serde(default)]
    pub name: Option<String>,
}

/// How Core wants a session started.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    /// The workspace directory, as a path on the session's host.
    pub cwd: String,
    /// The permission mode the backend starts in, in the backend's own words
    /// for now (for Claude Code: `default`, `acceptEdits`, `plan`, …). Passed
    /// explicitly, never left to the backend's default.
    pub permission_mode: String,
    /// The model to use; the backend's default when `None`.
    pub model: Option<String>,
    /// The effort to spend per turn, in the backend's own levels; the
    /// backend's default when `None`.
    pub effort: Option<String>,
    /// Whether the user trusts the session's project, so that the
    /// configuration in its workspace (settings, hooks, MCP servers) may load.
    /// False, the default, loads the user's own configuration only (ADR 0018).
    pub project_trusted: bool,
    /// The project's root, as a path on the session's host: the folder the
    /// user answered the trust question for. Empty when unknown.
    #[serde(default)]
    pub project_path: String,
    /// For a new session, the id the backend starts it under: the one Core
    /// took from [`Adapter::new_session_id`] and stored. `None` leaves the
    /// backend to choose, as it does for a backend without host-chosen ids.
    #[serde(default)]
    pub backend_session_id: Option<String>,
    /// For a fork: what it starts from. The new session has an id of its
    /// own, [`SessionConfig::backend_session_id`] or the one the backend
    /// reports.
    #[serde(default)]
    pub fork_of: Option<ForkOf>,
    /// To continue a session the backend already has a record of: the
    /// backend's id for it. The backend keeps that id and appends to the
    /// same record (FLOW-RESUME-SESSION-R01). `None` for a new session or a
    /// fork, which have [`SessionConfig::backend_session_id`] instead.
    #[serde(default)]
    pub resume: Option<String>,
    /// Backend-specific settings, read only by that backend's adapter. `null`
    /// when there are none.
    pub extra: Value,
}

/// The user's answer to a permission request, mirroring the parameters of
/// `session.decide_permission` in `PROTO-CORE-API`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissionDecision {
    pub behavior: PermissionBehavior,
    /// Shown to the agent, typically with a denial to say why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The tool input to run with instead of the requested one, when the user
    /// edited it before allowing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Map<String, Value>>,
    /// Indices into the request's `suggestions` the user chose to apply along
    /// with the decision, such as "always allow this".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apply_suggestions: Vec<u32>,
}

/// The user's reply to a question, mirroring the parameters of
/// `session.answer_question` in `PROTO-CORE-API`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QuestionReply {
    /// One answer per question, in `question.answered`'s shape.
    Answers(Vec<Answer>),
    /// The user chose not to answer.
    Declined,
}

/// Why an adapter call failed.
#[derive(Debug)]
pub enum AdapterError {
    /// Starting the backend or talking to it over the transport failed.
    Io(io::Error),
    /// The backend said something the adapter did not expect, such as a
    /// failed handshake.
    Protocol(String),
    /// The session's backend process is no longer running.
    NotRunning,
    /// `decide_permission` or `answer_question` named a request the backend
    /// is not waiting on, or one of the other kind.
    UnknownRequest(String),
}

// `Display` is the human-readable message; `Error` (below) marks the type as
// an error so it works with `?` into `Box<dyn Error>` and error-reporting crates.
impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdapterError::Io(e) => write!(f, "transport error: {e}"),
            AdapterError::Protocol(detail) => write!(f, "protocol error: {detail}"),
            AdapterError::NotRunning => write!(f, "the backend is not running"),
            AdapterError::UnknownRequest(id) => write!(f, "no pending request with id {id}"),
        }
    }
}

impl std::error::Error for AdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AdapterError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Lets `?` turn an `io::Error` from a transport call into an `AdapterError`.
impl From<io::Error> for AdapterError {
    fn from(e: io::Error) -> Self {
        AdapterError::Io(e)
    }
}
