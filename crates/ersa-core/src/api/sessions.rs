//! The `session.*` methods (`PROTO-CORE-API`, Sessions). Each takes the
//! parsed params and returns the typed result or an [`ApiError`]; the work
//! is the session manager's.

use std::time::Duration;

use ersa_adapter::{BackendChoices, Direction, PermissionDecision, SessionConfig};
use ersa_protocol::core_api::{
    AnswerQuestionParams, Attachment, AttachmentKind, Choice, DecidePermissionParams, Empty,
    IdParams, InspectParams, Inspected, RawDirection, RawLogEntry, RawLogParams, Session,
    SessionCreateParams, SessionSendParams, SessionSendResult, SetArchivedParams, SetModeParams,
    SetTitleParams, SubscribeParams, SubscribeResult, WorkspaceState,
};
use serde_json::Value;

use super::{ApiError, Peer};
use crate::attachments::{self, InspectError};
use crate::persistence::{self, LOCAL_HOST_ID, SessionRow};
use crate::state::Core;

/// The most raw lines one `session.raw_log` call returns, and the most bytes
/// of line text (one line always comes back, however long). A client pages
/// through a longer log by asking again from the last index it got plus one.
/// The log is held in memory, from when Core started the session's backend.
pub const RAW_LOG_LIMIT: usize = 1000;
pub const RAW_LOG_BYTES: usize = 8 * 1024 * 1024;

/// The longest title `session.set_title` takes: as much as Claude Code's
/// own session list shows.
pub const MAX_TITLE_CHARS: usize = 200;

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
        archived: false,
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

/// Restarting a session is milestone M1, and until then it is answered like
/// a reserved method, for an unknown id too. Core clears an archived
/// session's mark only once it accepts a resume, so a refused resume leaves
/// the session archived, as it was.
pub(super) async fn resume(_core: &Core, _params: IdParams) -> Result<Session, ApiError> {
    Err(ApiError::invalid_state(
        "session.resume is milestone M1 and not implemented yet",
    ))
}

/// `session.set_archived`: archiving stops a live session first, as
/// `session.stop` does.
pub(super) async fn set_archived(
    core: &Core,
    params: SetArchivedParams,
) -> Result<Session, ApiError> {
    Ok(core
        .sessions
        .set_archived(&params.id, params.archived)
        .await?)
}

pub(super) async fn stop(core: &Core, params: IdParams) -> Result<Session, ApiError> {
    find(core, &params.id).await?;
    core.sessions.stop(&params.id).await?;
    find(core, &params.id).await
}

pub(super) async fn remove(core: &Core, params: IdParams) -> Result<Session, ApiError> {
    Ok(core.sessions.remove(&params.id).await?)
}

/// `session.send`. `peer` is the connection's, whose lane for this session
/// opens once the send has its slot: an interrupt or an answer sent behind
/// it never waits for its attachments to be checked and copied.
pub(super) async fn send(
    core: &Core,
    peer: &Peer,
    params: SessionSendParams,
) -> Result<SessionSendResult, ApiError> {
    find(core, &params.id).await?;
    for (index, attachment) in params.attachments.iter().enumerate() {
        well_formed(attachment)
            .map_err(|e| ApiError::invalid_params(format!("attachments[{index}]: {e}")))?;
    }
    let turn = core
        .sessions
        .send(&params.id, params.text, params.attachments, || {
            peer.open_lane()
        })
        .await?;
    Ok(SessionSendResult { turn })
}

/// An attachment of one of the schema's two shapes: a path, or a pasted
/// image's `media_type` and `data`.
fn well_formed(attachment: &Attachment) -> Result<(), &'static str> {
    let pasted = attachment.media_type.is_some() || attachment.data.is_some();
    match (attachment.kind, &attachment.path, pasted) {
        (_, Some(_), false) => Ok(()),
        (AttachmentKind::Image, None, true)
            if attachment.media_type.is_some() && attachment.data.is_some() =>
        {
            Ok(())
        }
        (AttachmentKind::Image, _, _) => Err("an image takes a path, or media_type and data"),
        _ => Err("a file or folder takes a path"),
    }
}

/// How long `attachment.inspect` may take.
const INSPECT_LIMIT: Duration = Duration::from_secs(30);

/// The most inspects that may be reading a path at once, Core-wide. An
/// inspect answered at its limit leaves its thread waiting on the file
/// system, holding its place here until the read returns, so a path that
/// hangs, asked about again and again, takes this many threads, never all
/// of Core's.
static INSPECTING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

/// `attachment.inspect` (PROTO-CORE-API-R24), on the local host: the only
/// one Core reaches yet.
pub(super) async fn inspect(core: &Core, params: InspectParams) -> Result<Inspected, ApiError> {
    if params.host_id != LOCAL_HOST_ID {
        return Err(ApiError::not_found(format!("no host {}", params.host_id)));
    }
    let images = core.sessions.image_types();
    let path = params.path;
    let workspace = match params.workspace_id {
        Some(id) => {
            let workspace = core
                .store
                .workspace(id.clone())
                .await?
                .ok_or_else(|| ApiError::not_found(format!("no workspace {id}")))?;
            let project = core
                .store
                .project(workspace.project_id.clone())
                .await?
                .ok_or_else(|| {
                    ApiError::not_found(format!("no project {}", workspace.project_id))
                })?;
            same_host(&params.host_id, &project.host_id, &id)?;
            Some(workspace.path)
        }
        None => None,
    };
    let place = INSPECTING.try_acquire().map_err(|_| {
        ApiError::invalid_params(
            "too many paths asked about are not answering; try again once they do",
        )
    })?;
    // A blocking thread: a network drive that no longer answers holds that
    // thread, never the request, which is answered at the limit. The path is
    // read first and answered on its own, then whether sending copies it is
    // judged, so a workspace folder that stops answering never fails a path
    // that was read (PROTO-CORE-API-R44); the thread keeps its place for both.
    let (read_tx, read_rx) = tokio::sync::oneshot::channel();
    let (judged_tx, judged_rx) = tokio::sync::oneshot::channel();
    let judging = workspace.clone();
    let judged_path = path.clone();
    tokio::task::spawn_blocking(move || {
        let _place = place;
        let read = attachments::inspect(&path, &images);
        let judge = matches!(&read, Ok(i) if i.kind != AttachmentKind::Image);
        let _ = read_tx.send(read);
        if let (true, Some(workspace)) = (judge, judging) {
            let _ = judged_tx.send(attachments::copies(
                &judged_path,
                std::path::Path::new(&workspace),
            ));
        }
    });
    let mut inspected = match tokio::time::timeout(INSPECT_LIMIT, read_rx).await {
        Ok(Ok(Ok(inspected))) => inspected,
        Ok(Ok(Err(InspectError::NotFound(message)))) => return Err(ApiError::not_found(message)),
        Ok(Ok(Err(InspectError::Invalid(message)))) => {
            return Err(ApiError::invalid_params(message));
        }
        Ok(Err(e)) => return Err(ApiError::internal(format!("inspecting failed: {e}"))),
        Err(_) => {
            return Err(ApiError::invalid_params(format!(
                "the path did not answer within {INSPECT_LIMIT:?}"
            )));
        }
    };
    // An image goes inline, never copied; without a workspace, as a new one
    // not made yet, every file and folder is. A judgment that does not answer
    // in time is taken to be a copy, so the note never says one won't happen.
    inspected.copied = match (&inspected.kind, &workspace) {
        (AttachmentKind::Image, _) => false,
        (_, None) => true,
        (_, Some(_)) => tokio::time::timeout(JUDGE_LIMIT, judged_rx)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(true),
    };
    Ok(inspected)
}

/// How long `attachment.inspect` waits to judge whether sending copies a
/// path it has read, past which it is taken to be copied.
const JUDGE_LIMIT: Duration = Duration::from_secs(5);

/// A path is read on `host`, so it is judged only against a workspace there.
fn same_host(host: &str, workspace_host: &str, workspace_id: &str) -> Result<(), ApiError> {
    if host == workspace_host {
        Ok(())
    } else {
        Err(ApiError::invalid_params(format!(
            "workspace {workspace_id} is on host {workspace_host}, not {host}"
        )))
    }
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

/// Renames the session in its backend, which keeps the title where its own
/// tools show it too (ADR 0017, PROTO-CORE-API-R12).
pub(super) async fn set_title(core: &Core, params: SetTitleParams) -> Result<Session, ApiError> {
    // The title goes into the backend's record for good and shows in its own
    // tools, which list one line of at most 200 characters with every run of
    // spaces as one (PROTO-CLAUDE-CODE-STREAM, Listing). The title is taken
    // in that form, so Ersa shows what the backend's list will: a longer
    // title, or one holding a control character, would read differently
    // there. A line break or a tab is spacing, and collapses like any other.
    if params
        .title
        .chars()
        .any(|c| c.is_control() && !c.is_whitespace())
    {
        return Err(ApiError::invalid_params(
            "a session's title is one line, without control characters",
        ));
    }
    let title = params
        .title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return Err(ApiError::invalid_params(
            "a session's title cannot be empty",
        ));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(ApiError::invalid_params(format!(
            "a session's title is at most {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(core
        .sessions
        .set_title(&params.id, &title, |host| core.session_transport_for(host))
        .await?)
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

#[cfg(test)]
mod inspect_tests {
    use super::same_host;

    // R44-PLACEHOLDER: cites PROTO-CORE-API R44 once #243 merges
    #[test]
    fn a_workspace_on_another_host_is_invalid_params() {
        assert!(same_host("local", "local", "w-1").is_ok());
        let refused = same_host("local", "devbox", "w-1").unwrap_err();
        assert_eq!(refused.0.code, -32602, "{refused:?}");
    }
}
