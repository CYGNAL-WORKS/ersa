//! The Core request/response API (`PROTO-CORE-API`): everything between the
//! Interface and Core that is not an agent event.
//!
//! These types implement `protocol/schema/core-api.schema.json`, the source of
//! truth. `tests/core_api_conformance.rs` serializes examples of them and
//! validates each against the schema.
//!
//! - [`rpc`]: the JSON-RPC 2.0 framing every line is wrapped in.
//! - [`Method`]: every method name, and which are reserved for a later milestone.
//! - The rest of this module: each method's params and result, and each
//!   notification's params. Methods reserved for milestone M2 whose shapes the
//!   brief leaves open (`fs.*`, `terminal.*`) have no Rust types yet.
//!
//! The serde conventions are the ones in [`crate::events`]: optional fields are
//! omitted rather than written as `null`, and every closed object denies
//! unknown fields.

pub mod rpc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::events::{Answer, Envelope, LifecycleState, PermissionBehavior};

/// The Core API version this build speaks. Negotiated in `core.hello`; bumped
/// only for removals or changes of meaning, never for additions. The schema
/// file records the same number as `x-api-version`; a test checks they agree.
pub const API_VERSION: u32 = 2;

/// Every Core API version this build can speak, for the handshake.
pub const SUPPORTED_API_VERSIONS: &[u32] = &[API_VERSION];

// ---------------------------------------------------------------------------
// Methods

/// Declares [`Method`] from one list of `Variant => "wire.name"` pairs, so the
/// enum, its wire names, and [`Method::ALL`] cannot drift apart.
///
/// `macro_rules!` matches the pattern inside `( ... )*` once per pair and
/// pastes the body that many times. `$variant:ident` captures a Rust name and
/// `$name:literal` a string literal.
macro_rules! methods {
    ($($variant:ident => $name:literal,)*) => {
        /// A Core API method. `match`ing on this (rather than on the name
        /// string) makes the compiler check that Core handles every method.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Method {
            $($variant,)*
        }

        impl Method {
            /// Every method, in the order the brief lists them.
            pub const ALL: &'static [Method] = &[$(Method::$variant,)*];

            /// The method's name on the wire, `noun.verb`.
            pub fn name(self) -> &'static str {
                match self {
                    $(Method::$variant => $name,)*
                }
            }
        }
    };
}

methods! {
    CoreHello => "core.hello",
    CoreStatus => "core.status",
    CoreShutdown => "core.shutdown",
    HostList => "host.list",
    HostAdd => "host.add",
    HostRemove => "host.remove",
    HostProbe => "host.probe",
    ProjectList => "project.list",
    ProjectAdd => "project.add",
    ProjectRemove => "project.remove",
    WorkspaceList => "workspace.list",
    WorkspaceCreate => "workspace.create",
    WorkspaceRemove => "workspace.remove",
    SessionList => "session.list",
    SessionCreate => "session.create",
    SessionStart => "session.start",
    SessionStop => "session.stop",
    SessionResume => "session.resume",
    SessionRemove => "session.remove",
    SessionSend => "session.send",
    SessionInterrupt => "session.interrupt",
    SessionDecidePermission => "session.decide_permission",
    SessionAnswerQuestion => "session.answer_question",
    SessionSetTitle => "session.set_title",
    SessionSetMode => "session.set_mode",
    SessionSubscribe => "session.subscribe",
    SessionUnsubscribe => "session.unsubscribe",
    SessionRawLog => "session.raw_log",
    BackendList => "backend.list",
    ModeList => "mode.list",
    SettingsGet => "settings.get",
    SettingsSet => "settings.set",
    FsList => "fs.list",
    FsRead => "fs.read",
    FsDiff => "fs.diff",
    TerminalOpen => "terminal.open",
    TerminalInput => "terminal.input",
    TerminalResize => "terminal.resize",
    TerminalClose => "terminal.close",
}

impl Method {
    /// The method with this wire name, if there is one.
    pub fn from_name(name: &str) -> Option<Method> {
        Method::ALL.iter().copied().find(|m| m.name() == name)
    }

    /// The milestone a reserved method belongs to, or `None` for a method of
    /// this milestone. A reserved method answers `invalid_state` naming its
    /// milestone (PROTO-CORE-API-R10).
    pub fn reserved_for(self) -> Option<&'static str> {
        match self {
            Method::HostAdd | Method::HostRemove | Method::HostProbe => Some("M4"),
            Method::ModeList => Some("M3"),
            Method::FsList
            | Method::FsRead
            | Method::FsDiff
            | Method::TerminalOpen
            | Method::TerminalInput
            | Method::TerminalResize
            | Method::TerminalClose => Some("M2"),
            _ => None,
        }
    }
}

/// The notification method names Core sends.
pub mod notification {
    pub const SESSION_EVENT: &str = "session.event";
    pub const SESSION_TILE: &str = "session.tile";
    pub const HOST_STATUS: &str = "host.status";
    pub const CORE_NOTICE: &str = "core.notice";
    /// Reserved for milestone M2.
    pub const TERMINAL_OUTPUT: &str = "terminal.output";
}

// ---------------------------------------------------------------------------
// Errors

/// `data.kind` of an application error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NotFound,
    InvalidState,
    BackendMissing,
    BackendNotLoggedIn,
    VcsError,
    TransportError,
    Permission,
    Internal,
}

// ---------------------------------------------------------------------------
// Shared shapes

/// Params or a result with no fields: `{}`. `deny_unknown_fields` makes a
/// stray field an `invalid params` error rather than something ignored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// Params naming one object by its `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdParams {
    pub id: String,
}

// ---------------------------------------------------------------------------
// Core

/// `core.hello` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelloParams {
    /// What is attaching, such as `ersa-shell`.
    pub client: String,
    pub client_version: String,
    /// Every Core API version the client speaks.
    pub api_versions: Vec<u32>,
}

/// `core.hello` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelloResult {
    pub core_version: String,
    /// The highest version both sides speak.
    pub api_version: u32,
    /// The agent event schema version (`v`) this Core produces.
    pub event_projection_version: u32,
    pub host: HelloHost,
    pub capabilities: Vec<String>,
}

/// The host a Core runs on, as the handshake reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelloHost {
    pub id: String,
    pub name: String,
    /// `windows`, `macos`, or `linux`.
    pub platform: String,
}

/// `core.status` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusResult {
    pub sessions_running: u64,
    pub sessions_waiting: u64,
    pub uptime_s: u64,
    pub db_path: String,
}

// ---------------------------------------------------------------------------
// Hosts and backends

/// A machine where sessions can run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub kind: HostKind,
    pub status: HostStatus,
    pub backends: Vec<BackendStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Local,
    Ssh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostStatus {
    Connected,
    Disconnected,
    Unknown,
}

/// One backend on a host, as `host.list` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendStatus {
    /// The backend's name, such as `claude-code`.
    pub name: String,
    /// Absent when the backend is missing or its version could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub status: BackendState,
}

/// Whether a backend can be started on a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendState {
    /// Installed, and it reported a version.
    Available,
    /// Not found on the host.
    Missing,
    /// Found, but asking its version failed.
    Error,
}

/// `backend.list` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendListParams {
    pub host_id: String,
}

/// One backend on a host, with the range of its versions Ersa was tested against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub status: BackendState,
    pub min_tested_version: String,
    pub max_tested_version: String,
}

/// One entry of `mode.list` (reserved for milestone M3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeInfo {
    pub name: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Projects and workspaces

/// A repository or root folder added to Ersa, on one host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub host_id: String,
    /// The project's root, as a path on its host.
    pub path: String,
    pub name: String,
    pub vcs: Vcs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vcs {
    Git,
    Perforce,
    None,
}

/// `project.add` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAddParams {
    pub host_id: String,
    pub path: String,
}

/// An isolated checkout belonging to a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub id: String,
    pub project_id: String,
    /// The workspace's root, as a path on the project's host.
    pub path: String,
    /// The branch (git) the workspace has checked out.
    pub vcs_ref: String,
    pub state: WorkspaceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceState {
    Ready,
    Creating,
    Removing,
    Error,
}

/// `workspace.list` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceListParams {
    pub project_id: String,
}

/// `workspace.create` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceCreateParams {
    pub project_id: String,
    /// The branch name for a git worktree; Core picks one when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The commit-ish the workspace starts from; the project's HEAD when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

/// `workspace.remove` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRemoveParams {
    pub id: String,
    /// Remove even with uncommitted changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

// ---------------------------------------------------------------------------
// Sessions

/// A session as `session.list` reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub host_id: String,
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub state: LifecycleState,
    /// RFC 3339.
    pub created_at: String,
    /// Absent when the session has no events yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_seq: Option<u64>,
    pub tile: Tile,
}

/// What the rail renders for a session. Usage fields are absent until known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tile {
    /// The session's lifecycle state, so the rail follows it from
    /// `session.tile` notifications without subscribing to events.
    pub state: LifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd_total: Option<f64>,
    pub pending: Vec<PendingRequest>,
}

impl Tile {
    /// The tile of a session that has just been created: starting, with
    /// nothing known and nothing pending.
    pub fn starting() -> Self {
        Tile {
            state: LifecycleState::Starting,
            context_tokens: None,
            context_window: None,
            cost_usd_total: None,
            pending: Vec::new(),
        }
    }
}

/// A permission request or question waiting on the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingRequest {
    pub kind: PendingKind,
    pub request_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingKind {
    Permission,
    Question,
}

/// `session.create` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCreateParams {
    pub workspace_id: String,
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// `session.send` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSendParams {
    pub id: String,
    pub text: String,
}

/// `session.send` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSendResult {
    /// The 1-based turn the message starts.
    pub turn: u32,
}

/// `session.decide_permission` params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecidePermissionParams {
    pub id: String,
    pub request_id: String,
    pub behavior: PermissionBehavior,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Replacement tool input: the tool's own arguments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Map<String, Value>>,
    /// Indices into the request's suggestions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_suggestions: Option<Vec<u32>>,
}

/// `session.answer_question` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerQuestionParams {
    pub id: String,
    pub request_id: String,
    pub answers: Vec<Answer>,
}

/// `session.set_title` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTitleParams {
    pub id: String,
    pub title: String,
}

/// `session.set_mode` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetModeParams {
    pub id: String,
    pub mode: String,
}

/// `session.subscribe` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscribeParams {
    pub id: String,
    pub from_seq: u64,
}

/// `session.subscribe` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscribeResult {
    /// Absent when the session has no events yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<u64>,
    /// The identity of the view the subscription reads. A view is one
    /// projection of the session; when it changes, events a client holds
    /// from another view no longer line up, and it starts over.
    pub view: String,
    /// The first event the view still holds, when it no longer holds every
    /// event from 0: a subscription from before it starts here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<u64>,
}

/// `session.raw_log` params. Both bounds are inclusive raw-log indices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLogParams {
    pub id: String,
    /// 0 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<u64>,
    /// The end of the log when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<u64>,
}

/// One line of a session's raw log, for the developer pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLogEntry {
    pub index: u64,
    /// RFC 3339.
    pub ts: String,
    pub dir: RawDirection,
    /// The line verbatim, without its terminator.
    pub line: String,
}

/// Which way a raw-log line went: emitted by the backend, or written to it
/// by Ersa. Core's lifecycle facts are in its app log, not the raw log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawDirection {
    Backend,
    Host,
}

// ---------------------------------------------------------------------------
// Settings

/// `settings.set` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsSetParams {
    /// A JSON merge patch (RFC 7386): objects merge, `null` deletes a key.
    pub patch: Map<String, Value>,
}

// ---------------------------------------------------------------------------
// Notifications

/// `session.event` params: one agent event for a subscribed session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEventParams {
    pub session_id: String,
    pub event: Envelope,
}

/// `session.tile` params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionTileParams {
    pub session_id: String,
    pub tile: Tile,
}

/// `host.status` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostStatusParams {
    pub host_id: String,
    pub status: HostStatus,
}

/// `core.notice` params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoreNoticeParams {
    pub level: NoticeLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_round_trip() {
        for &method in Method::ALL {
            assert_eq!(Method::from_name(method.name()), Some(method));
        }
        assert_eq!(Method::from_name("core.nope"), None);
    }

    #[test]
    fn empty_params_reject_stray_fields() {
        assert!(serde_json::from_str::<Empty>("{}").is_ok());
        assert!(serde_json::from_str::<Empty>(r#"{"x":1}"#).is_err());
    }
}
