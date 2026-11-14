//! The agent event protocol (`PROTO-AGENT-EVENTS`): what happens in a session,
//! in Ersa's own words, independent of any backend.
//!
//! These types implement `protocol/schema/agent-events.schema.json`, which is the
//! source of truth. `tests/schema_conformance.rs` serializes one example of every
//! event type and validates it against that file, so a field added here but not
//! there (or the other way round) fails the build.
//!
//! Two serde conventions run through the whole module:
//!
//! - Optional fields are `Option<T>` with `skip_serializing_if = "Option::is_none"`,
//!   so an absent value is left out of the JSON rather than written as `null`.
//!   The schema has no `null`s anywhere; "absent" is the only way to say "unknown".
//! - Every struct has `deny_unknown_fields`, the deserializing twin of the
//!   schema's `additionalProperties: false`: a field this version does not know
//!   is an error, not something silently dropped (PROTO-AGENT-EVENTS-R08).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The event schema version this build produces and understands.
///
/// It is the `v` on every [`Envelope`] and the `const` on `v` in the schema;
/// a test checks the two agree. Bump it when any event's meaning or shape
/// changes, including the lifecycle events Core emits itself. Nothing
/// projected is stored (ADR 0017), so a bump needs no migration: Core
/// projects at its own `v` each time it opens a session.
///
/// 2: `source` names the input and the backend's message id instead of a
/// range of raw-log lines.
///
/// 3: `usage.updated` carries `turn_output_tokens`.
///
/// 4: `user.message` carries the message's `attachments`.
///
/// 5: `question.asked` carries each question's `header` and `takes_text` and
/// each option's `description`; `question.answered` carries `declined`, each
/// answer's `own_words`, and `answered_by: backend`.
///
/// 6: `tool.requested` carries `target` and `edits`.
pub const PROJECTION_VERSION: u32 = 6;

/// One event as Core streams it: a typed payload plus where it came from.
///
/// Adapters produce only the payload ([`Event`]); Core wraps it in an envelope,
/// assigning `seq` and taking `ts` from the input the event came from
/// (PROTO-AGENT-EVENTS-R01).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The event schema version; [`PROJECTION_VERSION`] for events this build made.
    pub v: u32,
    /// Ersa's session identifier, not the backend's.
    pub session_id: String,
    /// Gap-free and increasing within a session.
    pub seq: u64,
    /// RFC 3339 time of the input the event came from: the time the session
    /// record gives the entry, or the time Core received the live line. Never
    /// the clock at projection time, so projecting again gives the same value.
    pub ts: String,
    pub source: Source,
    pub event: Event,
}

/// Which backend produced an event, and from which input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub backend: String,
    pub backend_version: String,
    pub input: Input,
    /// The backend's id for the message the event came from. Events from the
    /// session record and the live stream are merged on it: a message in both
    /// yields its events once (PROTO-AGENT-EVENTS-R12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// What an event was projected from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Input {
    /// The backend's own session record: the session's history.
    Record,
    /// The live stream of a running backend, in either direction.
    Live,
    /// Core's own knowledge of the backend process: its lifecycle.
    Core,
}

/// The typed payload of an event, discriminated by `type`.
///
/// `#[serde(tag = "type")]` makes this an "internally tagged" enum: the variant
/// name is written as a `type` field inside the payload object, next to the
/// payload's own fields, which is exactly the wire shape the brief describes:
/// `{ "type": "turn.started", "turn": 1, "trigger": "user" }`.
///
/// Each variant wraps a struct of the same name, so other crates can build and
/// match payloads by name (`Event::TurnStarted(TurnStarted { .. })`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "session.started")]
    SessionStarted(SessionStarted),
    #[serde(rename = "session.state")]
    SessionState(SessionState),
    #[serde(rename = "session.ended")]
    SessionEnded(SessionEnded),
    #[serde(rename = "session.notice")]
    SessionNotice(SessionNotice),

    #[serde(rename = "turn.started")]
    TurnStarted(TurnStarted),
    #[serde(rename = "turn.completed")]
    TurnCompleted(TurnCompleted),

    #[serde(rename = "user.message")]
    UserMessage(UserMessage),

    #[serde(rename = "assistant.text")]
    AssistantText(AssistantText),
    #[serde(rename = "assistant.thinking")]
    AssistantThinking(AssistantThinking),

    #[serde(rename = "tool.requested")]
    ToolRequested(ToolRequested),
    #[serde(rename = "tool.result")]
    ToolResult(ToolResult),
    #[serde(rename = "tool.progress")]
    ToolProgress(ToolProgress),

    #[serde(rename = "permission.requested")]
    PermissionRequested(PermissionRequested),
    #[serde(rename = "permission.decided")]
    PermissionDecided(PermissionDecided),
    #[serde(rename = "permission.denied_unanswered")]
    PermissionDeniedUnanswered(PermissionDeniedUnanswered),

    #[serde(rename = "question.asked")]
    QuestionAsked(QuestionAsked),
    #[serde(rename = "question.answered")]
    QuestionAnswered(QuestionAnswered),

    #[serde(rename = "usage.updated")]
    UsageUpdated(UsageUpdated),

    #[serde(rename = "error")]
    Error(ErrorEvent),

    #[serde(rename = "backend.passthrough")]
    BackendPassthrough(BackendPassthrough),
}

impl Event {
    /// The wire name of this event's type, such as `"turn.started"`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Event::SessionStarted(_) => "session.started",
            Event::SessionState(_) => "session.state",
            Event::SessionEnded(_) => "session.ended",
            Event::SessionNotice(_) => "session.notice",
            Event::TurnStarted(_) => "turn.started",
            Event::TurnCompleted(_) => "turn.completed",
            Event::UserMessage(_) => "user.message",
            Event::AssistantText(_) => "assistant.text",
            Event::AssistantThinking(_) => "assistant.thinking",
            Event::ToolRequested(_) => "tool.requested",
            Event::ToolResult(_) => "tool.result",
            Event::ToolProgress(_) => "tool.progress",
            Event::PermissionRequested(_) => "permission.requested",
            Event::PermissionDecided(_) => "permission.decided",
            Event::PermissionDeniedUnanswered(_) => "permission.denied_unanswered",
            Event::QuestionAsked(_) => "question.asked",
            Event::QuestionAnswered(_) => "question.answered",
            Event::UsageUpdated(_) => "usage.updated",
            Event::Error(_) => "error",
            Event::BackendPassthrough(_) => "backend.passthrough",
        }
    }
}

// ---------------------------------------------------------------------------
// Session

/// The backend announced itself (Claude Code's `system/init`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionStarted {
    pub backend: String,
    pub backend_version: String,
    pub model: String,
    pub cwd: String,
    pub capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub state: LifecycleState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Where a session is in its lifecycle.
///
/// `rename_all = "snake_case"` writes `WaitingUser` as `"waiting_user"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Starting,
    Running,
    WaitingUser,
    Idle,
    Disconnected,
    Ended,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEnded {
    pub reason: EndReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndReason {
    UserStopped,
    BackendExited,
    Error,
}

/// Something the user should know that is not an error, such as a backend
/// newer than the newest version Ersa was tested against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionNotice {
    pub level: NoticeLevel,
    pub code: NoticeCode,
    pub message: String,
    /// With `session_open_elsewhere`: the process that has the session open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Warning,
}

/// What a notice is about. A closed list, so the Interface can word and place
/// each one deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeCode {
    /// The backend is newer than the newest version its adapter was tested with.
    BackendVersionUntested,
    /// Another process has the session open, so resuming or renaming it
    /// here writes to a record that process writes too.
    SessionOpenElsewhere,
    /// Whether another process has the session open could not be told.
    SessionOpenCheckFailed,
}

// ---------------------------------------------------------------------------
// Turn

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnStarted {
    /// 1-based index of the turn within the session.
    pub turn: u32,
    pub trigger: TurnTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnTrigger {
    User,
    Resume,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnCompleted {
    pub turn: u32,
    pub outcome: TurnOutcome,
    pub duration_ms: u64,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Success,
    Error,
    Interrupted,
    MaxTurns,
}

// ---------------------------------------------------------------------------
// User

/// A message to the agent, projected from the host line that sent it or from
/// the backend replaying it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserMessage {
    pub text: String,
    pub origin: MessageOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<Peer>,
    /// What came with the message, in the order sent; none when empty
    /// (PROTO-AGENT-EVENTS, `user.message`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MessageAttachment>,
}

/// One attachment of a user message: an image the agent saw, or a file or
/// folder by the path it was given, relative to the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessageAttachment {
    Image { media_type: String, data: String },
    File { path: String },
    Folder { path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    Human,
    PeerSession,
    System,
}

/// The session a `peer_session` message came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub session_id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Assistant

/// Assistant text. With partial streaming, several events share a `block_id`
/// with growing `text` and `is_final: false`, then one with `is_final: true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantText {
    pub block_id: String,
    pub text: String,
    /// `final` on the wire. `final` is a reserved word in Rust, so the field
    /// has a different name here and serde renames it.
    #[serde(rename = "final")]
    pub is_final: bool,
}

/// Thinking happened. Present even when the backend withholds the content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantThinking {
    pub block_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(rename = "final")]
    pub is_final: bool,
}

// ---------------------------------------------------------------------------
// Tool

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequested {
    pub tool_use_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The tool's arguments. Open: its keys are the tool's parameter names.
    pub input: Map<String, Value>,
    /// Set for calls made inside a subagent, pointing at the subagent's call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
    /// The one thing the call acts on, as the user would name it: a path, a
    /// command, a search or an address (PROTO-AGENT-EVENTS-R21). The adapter
    /// fills it from its backend's own tools, so the Interface never reads a
    /// backend's tool input to draw a card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// For a call that changes files, one entry per change in the order the
    /// call makes them (PROTO-AGENT-EVENTS-R20). Never an empty list: a call
    /// that changes no file carries none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edits: Option<Vec<FileEdit>>,
}

/// One change a tool call makes to a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEdit {
    /// The file's path, as the tool was given it.
    pub path: String,
    /// The text replaced; absent when the call writes the whole file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    pub new_text: String,
    /// Whether the change replaces `old_text` everywhere it occurs in the
    /// file, not once (PROTO-AGENT-EVENTS-R23). On the wire only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub every_occurrence: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: ToolResultContent,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// A tool's output: plain text, or structured output as the tool produced it.
///
/// `untagged` means no tag is written: `Text` serializes as a bare JSON string
/// and `Structured` as whatever value it holds. When reading, serde tries the
/// variants in order, so a string always becomes `Text`. The schema allows only
/// an array or object in `Structured`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Structured(Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolProgress {
    pub tool_use_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Permission

/// The agent asks the user to approve a tool call (PROTO-AGENT-EVENTS-R04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionRequested {
    pub request_id: String,
    pub tool_use_id: String,
    pub tool_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_path: Option<String>,
    pub suggestions: Vec<PermissionSuggestion>,
    pub flags: PermissionFlags,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<PermissionReason>,
}

/// Something the user can apply alongside "allow", such as "always allow this".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionSuggestion {
    pub kind: SuggestionKind,
    pub label: String,
    /// Backend-specific and opaque to the Interface; handed back to the adapter
    /// unchanged when the user applies the suggestion. One of the two places
    /// backend field names may appear (PROTO-AGENT-EVENTS-R08).
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    AllowRule,
    AllowDirectory,
    SetMode,
}

/// Hints for how the shelf presents a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionFlags {
    pub default_to_no: bool,
    pub suppress_always_allow: bool,
    pub requires_user_interaction: bool,
    pub classifier_approvable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionReason {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDecided {
    pub request_id: String,
    pub behavior: PermissionBehavior,
    pub decided_by: DecidedBy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Indices into the request's `suggestions`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied_suggestions: Option<Vec<u32>>,
}

/// Allow or deny. Shared with the Core API's `session.decide_permission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionBehavior {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    User,
    Rule,
    Backend,
    Timeout,
}

/// The backend denied a tool call because nobody could answer. A defect signal:
/// it should not occur in a correctly configured session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDeniedUnanswered {
    pub tool_use_id: String,
    pub tool_name: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Question

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionAsked {
    pub request_id: String,
    pub questions: Vec<Question>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    /// Unique within the request.
    pub id: String,
    pub text: String,
    /// A short label for the question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<QuestionOption>>,
    pub multi_select: bool,
    /// The user may answer in their own words beside the options.
    pub takes_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionAnswered {
    pub request_id: String,
    /// One per question; empty when the user declined or the backend
    /// withdrew the question.
    pub answers: Vec<Answer>,
    /// The user chose not to answer.
    pub declined: bool,
    pub answered_by: AnsweredBy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    /// The `id` of the question this answers.
    pub id: String,
    pub value: AnswerValue,
    /// `value` is the user's own words, not option ids.
    pub own_words: bool,
}

/// The user's own words or one option id; several option ids for a
/// multi-select question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnswerValue {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnsweredBy {
    User,
    Timeout,
    /// The backend withdrew the question, after an interrupt say.
    Backend,
}

// ---------------------------------------------------------------------------
// Usage

/// New usage figures. A field left `None` means "no new information", not zero:
/// the Interface keeps the last known value of each field.
///
/// Only `PartialEq`, not `Eq`: `f64` cannot be `Eq` because `NaN != NaN`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageUpdated {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd_total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turns: Option<u32>,
    /// Output tokens generated so far in the current turn, across all of its
    /// API calls (PROTO-AGENT-EVENTS-R17). Only live input carries it: a turn
    /// read from the session record has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimit {
    pub status: String,
    /// RFC 3339 time at which the limit resets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Error

/// Something the user could act on went wrong. Named `ErrorEvent` rather than
/// `Error` so it does not shadow `std::error::Error` where both are in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorEvent {
    pub code: ErrorCode,
    pub message: String,
    pub recoverable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<ErrorAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    AuthenticationFailed,
    BackendExited,
    Protocol,
    BackendError,
    Transport,
}

/// What the Interface offers the user to recover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorAction {
    LogIn,
    Restart,
    Reconnect,
}

// ---------------------------------------------------------------------------
// Passthrough

/// A raw line that maps to no other event (PROTO-AGENT-EVENTS-R02). Shown only
/// in the developer pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendPassthrough {
    pub backend: String,
    /// The backend's own type and subtype joined with `/`, such as `system/hook`.
    pub kind: String,
    /// The raw line's JSON value, or the verbatim line as a string when it did
    /// not parse. Open by design (PROTO-AGENT-EVENTS-R08).
    pub payload: Value,
}
