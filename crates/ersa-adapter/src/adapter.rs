//! The traits an adapter implements, and the one Core implements for it.

use std::fmt;
use std::io;
use std::sync::Arc;

use async_trait::async_trait;
use ersa_protocol::events::{Event, PermissionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::raw::{Direction, RawLine};
use crate::transport::{ProcessExit, Transport};

/// One backend's adapter. Core holds one of each in its adapter registry.
#[async_trait]
pub trait Adapter: Send + Sync {
    /// The backend's name, as used in `source.backend` and the Core API, such
    /// as `"claude-code"`.
    fn backend(&self) -> &str;

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

    /// A fresh projector for one session's raw log. Core makes one per
    /// projection pass and feeds it the log from the first line.
    fn projector(&self) -> Box<dyn Projector>;
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

    /// Answers the backend's pending permission request `request_id`, the id
    /// carried by its `permission.requested` event.
    async fn decide_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), AdapterError>;

    /// Asks the backend to stop the current turn. The session stays alive.
    async fn interrupt(&self) -> Result<(), AdapterError>;

    /// Ends the backend process, gracefully if the backend supports it and by
    /// killing it otherwise, and returns once it has exited.
    async fn stop(&self) -> Result<(), AdapterError>;
}

/// Where an adapter delivers what crosses the backend's stdio. Core implements
/// it; the adapter never sees the raw log or the event stream.
///
/// The methods are synchronous and must return quickly: Core stamps the line
/// and queues it for storage, never blocking on I/O. That lets an adapter call
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

/// Turns one session's raw log into Ersa events (`PROTO-AGENT-EVENTS`,
/// Projection rules).
///
/// A projector is a pure function of the lines it is fed, in order: no clock,
/// no I/O, no randomness, so the same log always gives the same events
/// (PROTO-AGENT-EVENTS-R03). It may keep state between lines (`&mut self`), such
/// as the text of a block being streamed or the turn number.
pub trait Projector: Send {
    /// The version of this adapter's projection logic. Core caches projections
    /// keyed by it and re-projects when it changes, so bump it whenever the
    /// events this projector produces for some log would change.
    fn version(&self) -> u32;

    /// The events `line` completes. Every line yields at least one, with
    /// `backend.passthrough` as the fallback (PROTO-AGENT-EVENTS-R02), and a
    /// line that cannot be projected yields a recoverable `error` plus a
    /// passthrough of the line, never a panic (PROTO-AGENT-EVENTS-R10).
    ///
    /// Payloads only: Core wraps each in an envelope, assigning `seq`, taking
    /// `ts` from `line`, and setting `source.raw_to` to `line.index`.
    fn project(&mut self, line: &RawLine) -> Vec<Projected>;
}

/// One event a projector produced, with the first raw line it came from.
///
/// The envelope's `source` names an inclusive range of raw lines. The last is
/// always the line being projected; the first is earlier when the event
/// gathers several lines, as streamed text does. Only the projector knows that,
/// so it says so here.
#[derive(Debug, Clone, PartialEq)]
pub struct Projected {
    /// Index of the first raw line the event came from; at most the index of
    /// the line being projected.
    pub raw_from: u64,
    pub event: Event,
}

impl Projected {
    /// An event that came from `line` alone, the common case.
    pub fn from_line(line: &RawLine, event: Event) -> Self {
        Projected {
            raw_from: line.index,
            event,
        }
    }
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
    /// `decide_permission` named a request the backend is not waiting on.
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
