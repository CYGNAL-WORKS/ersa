//! Core API: the request/response surface the Interface uses, for projects,
//! hosts, workspaces, sessions, settings, files, and diffs (`PROTO-CORE-API`).
//! The wire types live in `ersa-protocol`.
//!
//! - [`server`]: listens on the socket and runs one task per connection.
//! - This module: turns one request into one response, enforcing the
//!   handshake and the reserved methods.
//! - `handlers`: one function per implemented method.

mod handlers;
mod lines;
pub mod server;
mod sessions;

use ersa_protocol::core_api::rpc::{ErrorObject, Request, Response, code};
use ersa_protocol::core_api::{ErrorKind, Method};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::mpsc;

use std::collections::HashMap;

use ersa_adapter::AdapterError;

use crate::persistence;
use crate::sessions::{Kick, SessionError, Subscription};
use crate::state::Core;
use crate::workspace::VcsError;

/// A failed request: the JSON-RPC error to answer with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError(pub ErrorObject);

impl ApiError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        ApiError(ErrorObject::application(kind, message))
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidState, message)
    }

    /// `params` did not fit the method. A protocol-level error: the request
    /// itself was wrong, not the state of anything in Core.
    pub fn invalid_params(message: impl Into<String>) -> Self {
        ApiError(ErrorObject::protocol(code::INVALID_PARAMS, message))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }

    pub fn with_detail(self, detail: impl Into<String>) -> Self {
        ApiError(self.0.with_detail(detail))
    }
}

impl From<persistence::Error> for ApiError {
    fn from(e: persistence::Error) -> Self {
        match e {
            persistence::Error::InvalidSettings(message) => ApiError::invalid_params(message),
            // Another request got there first; retrying sees the new state.
            persistence::Error::Conflict(message) => ApiError::invalid_state(message),
            other => {
                // Internal errors are logged in full here and reported
                // briefly, so nothing is swallowed (ARCH-CORE, Errors).
                tracing::error!(error = %other, "database failure");
                ApiError::internal(other.to_string())
            }
        }
    }
}

impl From<VcsError> for ApiError {
    fn from(e: VcsError) -> Self {
        match e {
            VcsError::Spawn(_) | VcsError::Incomplete(_) => {
                ApiError::new(ErrorKind::TransportError, e.to_string())
            }
            VcsError::Failed { message, stderr } => {
                ApiError::new(ErrorKind::VcsError, message).with_detail(stderr)
            }
            VcsError::Locked { .. } | VcsError::Dirty | VcsError::NotListed { .. } => {
                ApiError::new(ErrorKind::VcsError, e.to_string())
            }
        }
    }
}

impl From<SessionError> for ApiError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::NotFound(id) => ApiError::not_found(format!("no session {id}")),
            SessionError::InvalidState(message) => ApiError::invalid_state(message),
            SessionError::BackendMissing(message) => {
                ApiError::new(ErrorKind::BackendMissing, message)
            }
            SessionError::Store(e) => e.into(),
            SessionError::Adapter(e) => match e {
                AdapterError::NotRunning => ApiError::invalid_state("the session is not running"),
                AdapterError::UnknownRequest(id) => {
                    ApiError::not_found(format!("no pending request {id}"))
                }
                AdapterError::Protocol(message) => ApiError::invalid_state(message),
                AdapterError::Io(e) => ApiError::new(ErrorKind::TransportError, e.to_string()),
            },
            SessionError::Internal(message) => {
                tracing::error!(%message, "session failure");
                ApiError::internal(message)
            }
        }
    }
}

/// What one connection knows about itself.
pub struct ConnectionState {
    /// The API version agreed in `core.hello`; `None` until then.
    pub api_version: Option<u32>,
    /// Lines to write to this connection's client. Responses and
    /// notifications both go through it, which is why it is a channel:
    /// notifications are written while requests are being handled.
    pub outbound: mpsc::Sender<String>,
    /// Raised to close the connection: Core does so when the client cannot
    /// keep up with its notifications.
    pub kick: Kick,
    /// This connection's subscriptions, by session id. Dropping one (or the
    /// connection) ends it; the session itself carries on.
    pub subscriptions: HashMap<String, Subscription>,
    /// Forwards `session.tile` notifications once the handshake is done.
    tile_task: Option<server::AbortOnDrop>,
}

impl ConnectionState {
    pub fn new(outbound: mpsc::Sender<String>, kick: Kick) -> Self {
        ConnectionState {
            api_version: None,
            outbound,
            kick,
            subscriptions: HashMap::new(),
            tile_task: None,
        }
    }
}

/// What the connection does after answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Continue,
    /// Close this connection (the handshake failed).
    Close,
    /// Close this connection and stop Core (`core.shutdown`).
    Shutdown,
}

/// Answers one request.
pub async fn handle(
    core: &Core,
    conn: &mut ConnectionState,
    request: Request,
) -> (Response, After) {
    let id = request.id.clone();
    match dispatch(core, conn, request).await {
        Ok((result, after)) => (Response::success(id, result), after),
        Err((ApiError(error), after)) => (Response::failure(Some(id), error), after),
    }
}

type Answer = Result<(Value, After), (ApiError, After)>;

async fn dispatch(core: &Core, conn: &mut ConnectionState, request: Request) -> Answer {
    // Before the handshake, every method other than core.hello is refused,
    // known or not (PROTO-CORE-API-R01).
    if conn.api_version.is_none() && request.method != Method::CoreHello.name() {
        return Err((
            ApiError::invalid_state(format!(
                "the first request on a connection must be core.hello, not {}",
                request.method
            )),
            After::Continue,
        ));
    }
    let Some(method) = Method::from_name(&request.method) else {
        return Err((
            ApiError(ErrorObject::protocol(
                code::METHOD_NOT_FOUND,
                format!("no method {}", request.method),
            )),
            After::Continue,
        ));
    };
    if let Some(milestone) = method.reserved_for() {
        // A reserved method is part of the API and gets an application
        // error, never "method not found" (PROTO-CORE-API-R10).
        return Err((
            ApiError::invalid_state(format!(
                "{} is reserved for milestone {milestone} and not implemented yet",
                method.name()
            )),
            After::Continue,
        ));
    }

    let params = request.params;
    // Each arm parses its params, runs its handler, and serializes the result.
    // `?` returns early with an error that keeps the connection open.
    let result = match method {
        Method::CoreHello => return handlers::core_hello(core, conn, parse(params)?),
        Method::CoreShutdown => {
            let _: ersa_protocol::core_api::Empty = parse(params)?;
            // The connection writes this answer, then requests the shutdown.
            return Ok((serde_json::json!({}), After::Shutdown));
        }
        Method::CoreStatus => to_json(handlers::core_status(core, parse(params)?).await),
        Method::HostList => to_json(handlers::host_list(core, parse(params)?).await),
        Method::BackendList => to_json(handlers::backend_list(core, parse(params)?).await),
        Method::ProjectList => to_json(handlers::project_list(core, parse(params)?).await),
        Method::ProjectAdd => to_json(handlers::project_add(core, parse(params)?).await),
        Method::ProjectRemove => to_json(handlers::project_remove(core, parse(params)?).await),
        Method::WorkspaceList => to_json(handlers::workspace_list(core, parse(params)?).await),
        Method::WorkspaceCreate => to_json(handlers::workspace_create(core, parse(params)?).await),
        Method::WorkspaceRemove => to_json(handlers::workspace_remove(core, parse(params)?).await),
        Method::SettingsGet => to_json(handlers::settings_get(core, parse(params)?).await),
        Method::SettingsSet => to_json(handlers::settings_set(core, parse(params)?).await),

        Method::SessionList => to_json(sessions::list(core, parse(params)?).await),
        Method::SessionCreate => to_json(sessions::create(core, parse(params)?).await),
        Method::SessionStart => to_json(sessions::start(core, parse(params)?).await),
        Method::SessionStop => to_json(sessions::stop(core, parse(params)?).await),
        Method::SessionResume => to_json(sessions::resume(core, parse(params)?).await),
        Method::SessionRemove => to_json(sessions::remove(core, parse(params)?).await),
        Method::SessionSend => to_json(sessions::send(core, parse(params)?).await),
        Method::SessionInterrupt => to_json(sessions::interrupt(core, parse(params)?).await),
        Method::SessionDecidePermission => {
            to_json(sessions::decide_permission(core, parse(params)?).await)
        }
        Method::SessionAnswerQuestion => {
            to_json(sessions::answer_question(core, parse(params)?).await)
        }
        Method::SessionSetTitle => to_json(sessions::set_title(core, parse(params)?).await),
        Method::SessionSetMode => to_json(sessions::set_mode(core, parse(params)?).await),
        Method::SessionSubscribe => to_json(sessions::subscribe(core, conn, parse(params)?)),
        Method::SessionUnsubscribe => to_json(sessions::unsubscribe(conn, parse(params)?)),
        Method::SessionRawLog => to_json(sessions::raw_log(core, parse(params)?).await),
        Method::SessionReproject => to_json(sessions::reproject(core, parse(params)?).await),

        // Handled above by `reserved_for`; listed so that adding a method
        // without deciding how to answer it is a compile error.
        Method::HostAdd
        | Method::HostRemove
        | Method::HostProbe
        | Method::ModeList
        | Method::FsList
        | Method::FsRead
        | Method::FsDiff
        | Method::TerminalOpen
        | Method::TerminalInput
        | Method::TerminalResize
        | Method::TerminalClose => unreachable!("reserved methods are answered above"),
    };
    result
        .map(|value| (value, After::Continue))
        .map_err(|e| (e, After::Continue))
}

/// Parses a method's params. Absent params count as `{}`, so methods without
/// parameters can be called without them.
fn parse<T: DeserializeOwned>(params: Option<Value>) -> Result<T, (ApiError, After)> {
    let params = params.unwrap_or_else(|| Value::Object(Default::default()));
    serde_json::from_value(params).map_err(|e| {
        (
            ApiError::invalid_params(format!("invalid params: {e}")),
            After::Continue,
        )
    })
}

fn to_json<T: Serialize>(result: Result<T, ApiError>) -> Result<Value, ApiError> {
    let value = result?;
    serde_json::to_value(value).map_err(|e| ApiError::internal(format!("serializing: {e}")))
}
