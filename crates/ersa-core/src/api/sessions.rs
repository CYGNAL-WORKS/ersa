//! The `session.*` methods (`PROTO-CORE-API`, Sessions). Each takes the
//! parsed params and returns the typed result or an [`ApiError`]; the work
//! is the session manager's.

use ersa_adapter::{BackendChoices, Direction, PermissionDecision, SessionConfig};
use ersa_protocol::core_api::{
    AnswerQuestionParams, Choice, DecidePermissionParams, Empty, IdParams, RawDirection,
    RawLogEntry, RawLogParams, Session, SessionCreateParams, SessionSendParams, SessionSendResult,
    SetModeParams, SetTitleParams, SubscribeParams, SubscribeResult, WorkspaceState,
};
use serde_json::Value;

use super::{ApiError, Peer};
use crate::persistence::{self, LOCAL_HOST_ID, SessionRow};
use crate::state::Core;

/// The most raw lines one `session.raw_log` call returns, and the most bytes
/// of line text (one line always comes back, however long). A client pages
/// through a longer log by asking again from the last index it got plus one.
/// The log is held in memory, from when Core started the session's backend.
pub const RAW_LOG_LIMIT: usize = 1000;
pub const RAW_LOG_BYTES: usize = 8 * 1024 * 1024;

pub(super) async fn list(core: &Core, _: Empty) -> Result<Vec<Session>, ApiError> {
    Ok(core.sessions.list())
}

async fn find(core: &Core, id: &str) -> Result<Session, ApiError> {
    Ok(core.sessions.get(id)?)
}

pub(super) async fn create(core: &Core, params: SessionCreateParams) -> Result<Session, ApiError> {
    let workspace = core
        .store
        .workspace(params.workspace_id.clone())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no workspace {}", params.workspace_id)))?;
    if workspace.state != WorkspaceState::Ready {
        return Err(ApiError::invalid_state(format!(
            "workspace {} is not ready",
            workspace.id
        )));
    }
    let project = core
        .store
        .project(workspace.project_id.clone())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no project {}", workspace.project_id)))?;
    let Some(adapter) = core.sessions.adapter(&params.backend) else {
        return Err(ApiError::new(
            ersa_protocol::core_api::ErrorKind::BackendMissing,
            format!("Ersa has no adapter for backend {}", params.backend),
        ));
    };
    // Only what `backend.list` offers: anything else would reach the
    // backend's command line.
    let choices = adapter.choices();
    check_choice("model", &params.model, &choices.models)?;
    check_choice("effort", &params.effort, &choices.efforts)?;
    check_choice(
        "permission_mode",
        &params.permission_mode,
        &choices.permission_modes,
    )?;
    // The permission mode is always stated to the backend: the named one,
    // else the backend's default, which the session then started with.
    let permission_mode = params
        .permission_mode
        .clone()
        .unwrap_or_else(|| BackendChoices::default_of(&choices.permission_modes));

    let transport = core
        .session_transport_for(&project.host_id)
        .ok_or_else(|| {
            ApiError::new(
                ersa_protocol::core_api::ErrorKind::TransportError,
                format!("host {} cannot be reached", project.host_id),
            )
        })?;
    // The version detection found, until the backend announces its own.
    let backend_version = if project.host_id == LOCAL_HOST_ID {
        core.local_backends()
            .await
            .into_iter()
            .find(|d| d.backend.name == params.backend)
            .and_then(|d| d.version)
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Trust is read again here, after the detection above, which can take
    // seconds: an answer the user changed meanwhile is the one that counts.
    let trusted = core
        .store
        .project(project.id.clone())
        .await?
        .is_some_and(|p| p.trusted);

    let session = SessionRow {
        id: persistence::new_id("s"),
        workspace_id: workspace.id,
        host_id: project.host_id,
        backend: params.backend,
        backend_session_id: None,
        mode: params.mode,
        cwd: workspace.path.clone(),
        // Only the model and effort named; one left out is the backend's to
        // choose (PROTO-CORE-API-R16).
        model: params.model.clone(),
        effort: params.effort.clone(),
        permission_mode: (!permission_mode.is_empty()).then(|| permission_mode.clone()),
    };
    let config = SessionConfig {
        cwd: workspace.path,
        // Modes are presets of their own (milestone M3), not the backend's
        // permission mode.
        permission_mode,
        // Passed only when named, so the backend's own defaults apply
        // otherwise (PROTO-CLAUDE-CODE-STREAM, Invocation).
        model: params.model,
        effort: params.effort,
        // The project's own configuration loads only when the user trusts
        // it (ADR 0018).
        project_trusted: trusted,
        // What the user answered the trust question for; a backend that
        // keeps its own trust record is told of it (ADR 0018).
        project_path: project.path.clone(),
        extra: Value::Null,
    };
    Ok(core
        .sessions
        .create(session, transport, config, backend_version)
        .await?)
}

/// Refuses a `field` value that is not one of `offered`; absent is the
/// backend's default.
fn check_choice(field: &str, value: &Option<String>, offered: &[Choice]) -> Result<(), ApiError> {
    match value {
        Some(id) if !offered.iter().any(|c| &c.id == id) => {
            let ids: Vec<&str> = offered.iter().map(|c| c.id.as_str()).collect();
            Err(ApiError::invalid_params(format!(
                "{field} {id:?} is not one this backend offers: [{}]",
                ids.join(", ")
            )))
        }
        _ => Ok(()),
    }
}

pub(super) async fn start(core: &Core, params: IdParams) -> Result<Session, ApiError> {
    find(core, &params.id).await?;
    if core.sessions.is_live(&params.id)? {
        return Err(ApiError::invalid_state(format!(
            "session {} is already running",
            params.id
        )));
    }
    Err(ApiError::invalid_state(
        "starting a stopped session again is resuming it, which is milestone M1",
    ))
}

/// Answered like a reserved method until its milestone, whatever the id.
pub(super) async fn resume(_: &Core, _: IdParams) -> Result<Session, ApiError> {
    Err(ApiError::invalid_state(
        "session.resume is milestone M1 and not implemented yet",
    ))
}

pub(super) async fn stop(core: &Core, params: IdParams) -> Result<Session, ApiError> {
    find(core, &params.id).await?;
    core.sessions.stop(&params.id).await?;
    find(core, &params.id).await
}

pub(super) async fn remove(core: &Core, params: IdParams) -> Result<Session, ApiError> {
    Ok(core.sessions.remove(&params.id).await?)
}

pub(super) async fn send(
    core: &Core,
    params: SessionSendParams,
) -> Result<SessionSendResult, ApiError> {
    find(core, &params.id).await?;
    let turn = core.sessions.send(&params.id, &params.text).await?;
    Ok(SessionSendResult { turn })
}

pub(super) async fn interrupt(core: &Core, params: IdParams) -> Result<Session, ApiError> {
    find(core, &params.id).await?;
    core.sessions.interrupt(&params.id).await?;
    find(core, &params.id).await
}

pub(super) async fn decide_permission(
    core: &Core,
    params: DecidePermissionParams,
) -> Result<Session, ApiError> {
    find(core, &params.id).await?;
    let decision = PermissionDecision {
        behavior: params.behavior,
        message: params.message,
        updated_input: params.updated_input,
        apply_suggestions: params.apply_suggestions.unwrap_or_default(),
    };
    core.sessions
        .decide_permission(&params.id, &params.request_id, decision)
        .await?;
    find(core, &params.id).await
}

pub(super) async fn answer_question(
    core: &Core,
    params: AnswerQuestionParams,
) -> Result<Session, ApiError> {
    let session = find(core, &params.id).await?;
    // No adapter projects `question.asked` yet, so no question can be
    // pending.
    Err(ApiError::invalid_state(format!(
        "the {} adapter does not support questions yet",
        session.backend
    )))
}

/// Refused: a session's title is its backend's to keep, in the session
/// record, where the backend's own tools show it too (ADR 0017).
pub(super) async fn set_title(core: &Core, params: SetTitleParams) -> Result<Session, ApiError> {
    find(core, &params.id).await?;
    Err(title_refused())
}

fn title_refused() -> ApiError {
    ApiError::invalid_state(
        "a session's title comes from its backend's session record; rename it there (in Claude Code, /rename)",
    )
}

/// Takes effect on the session's next start.
pub(super) async fn set_mode(core: &Core, params: SetModeParams) -> Result<Session, ApiError> {
    Ok(core.sessions.set_mode(&params.id, params.mode).await?)
}

/// Subscribes this connection to a session's events; a second subscription
/// to the same session replaces the first. Opens the session first when it
/// needs opening, which reads its record on its host.
pub(super) async fn subscribe(
    core: &Core,
    peer: &Peer,
    params: SubscribeParams,
) -> Result<SubscribeResult, ApiError> {
    let outbound = peer
        .outbound
        .upgrade()
        .ok_or_else(|| ApiError::invalid_state("the connection is closing"))?;
    let (subscription, subscribed) = core
        .sessions
        .subscribe(
            &params.id,
            params.from_seq,
            |host| core.session_transport_for(host),
            outbound,
            peer.kick.clone(),
        )
        .await?;
    // A replay starts playing once someone is watching it.
    crate::replay::subscribed(core);
    // Only now is the old subscription, if any, dropped: a resubscribe that
    // fails leaves the client the stream it had. It is dropped outside the
    // lock.
    let old = peer
        .subscriptions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(params.id, subscription);
    drop(old);
    Ok(SubscribeResult {
        last_seq: subscribed.last_seq,
        view: subscribed.view,
        first_seq: subscribed.first_seq,
    })
}

pub(super) fn unsubscribe(peer: &Peer, params: IdParams) -> Result<Empty, ApiError> {
    let old = peer
        .subscriptions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&params.id);
    drop(old);
    Ok(Empty {})
}

/// Raw lines `from..=to`, exactly as they crossed (PROTO-CORE-API-R08), at
/// most [`RAW_LOG_LIMIT`] of them.
pub(super) async fn raw_log(
    core: &Core,
    params: RawLogParams,
) -> Result<Vec<RawLogEntry>, ApiError> {
    let lines = core.sessions.raw_log(
        &params.id,
        params.from.unwrap_or(0),
        params.to.unwrap_or(u64::MAX),
        RAW_LOG_LIMIT,
        RAW_LOG_BYTES,
    )?;
    Ok(lines
        .into_iter()
        .map(|line| RawLogEntry {
            index: line.index,
            ts: line.ts,
            dir: match line.dir {
                Direction::Backend => RawDirection::Backend,
                Direction::Host => RawDirection::Host,
            },
            line: line.line,
        })
        .collect())
}
