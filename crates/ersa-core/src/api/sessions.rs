//! The `session.*` methods (`PROTO-CORE-API`, Sessions). Each takes the
//! parsed params and returns the typed result or an [`ApiError`]; the work
//! is the session manager's.

use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{
    BackendChoices, Direction, PermissionDecision, QuestionReply, SessionConfig, Transport,
};
use ersa_protocol::core_api::{
    AnswerQuestionParams, ArrangeParams, Attachment, AttachmentKind, Choice,
    DecidePermissionParams, Empty, IdParams, InspectParams, Inspected, Project, RawDirection,
    RawLogEntry, RawLogParams, Session, SessionCreateParams, SessionSendParams, SessionSendResult,
    SetArchivedParams, SetModeParams, SetTitleParams, SubscribeParams, SubscribeResult, Workspace,
    WorkspaceState,
};
use serde_json::Value;

use super::{ApiError, Peer};
use crate::attachments::{self, InspectError};
use crate::persistence::{self, LOCAL_HOST_ID, SessionRow};
use crate::sessions::Forkable;
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

/// What starting a session in a workspace takes, looked up the same way
/// for a new session and a fork.
struct Starting {
    transport: Arc<dyn Transport>,
    /// The version detection found, until the backend announces its own.
    backend_version: String,
}

/// The workspace `id`, which must be `ready`, and its project.
async fn ready_workspace(core: &Core, id: &str) -> Result<(Workspace, Project), ApiError> {
    let workspace = core
        .store
        .workspace(id.to_string())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no workspace {id}")))?;
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
    Ok((workspace, project))
}

async fn starting(core: &Core, project: &Project, backend: &str) -> Result<Starting, ApiError> {
    let transport = core
        .session_transport_for(&project.host_id)
        .ok_or_else(|| {
            ApiError::new(
                ersa_protocol::core_api::ErrorKind::TransportError,
                format!("host {} cannot be reached", project.host_id),
            )
        })?;
    let backend_version = if project.host_id == LOCAL_HOST_ID {
        core.local_backends()
            .await
            .into_iter()
            .find(|d| d.backend.name == backend)
            .and_then(|d| d.version)
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Starting {
        transport,
        backend_version,
    })
}

/// Whether the user trusts the project now. Read last before a session
/// starts, after whatever may have taken seconds: an answer the user changed
/// meanwhile is the one that counts.
async fn trusted_now(core: &Core, project: &Project) -> Result<bool, ApiError> {
    Ok(core
        .store
        .project(project.id.clone())
        .await?
        .is_some_and(|p| p.trusted))
}

pub(super) async fn create(core: &Core, params: SessionCreateParams) -> Result<Session, ApiError> {
    let (workspace, project) = ready_workspace(core, &params.workspace_id).await?;
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

    let Starting {
        transport,
        backend_version,
    } = starting(core, &project, &params.backend).await?;
    let trusted = trusted_now(core, &project).await?;

    // A backend that starts a session under an id Ersa chooses gets one
    // now: stored with the row before the backend starts, so the session can
    // be renamed, and its record found, from the moment it exists
    // (PROTO-CLAUDE-CODE-STREAM-R35). Any other backend names it as it
    // starts.
    let backend_session_id = adapter.new_session_id();
    let session = SessionRow {
        id: persistence::new_id("s"),
        workspace_id: workspace.id,
        host_id: project.host_id,
        backend: params.backend,
        backend_session_id: backend_session_id.clone(),
        mode: params.mode,
        cwd: workspace.path.clone(),
        // Only the model and effort named; one left out is the backend's to
        // choose (PROTO-CORE-API-R16).
        model: params.model.clone(),
        effort: params.effort.clone(),
        permission_mode: (!permission_mode.is_empty()).then(|| permission_mode.clone()),
        archived: false,
        deleting: None,
        pinned: false,
        section_id: None,
        last_activity_at: None,
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
        backend_session_id,
        fork_of: None,
        extra: Value::Null,
    };
    Ok(core
        .sessions
        .create(session, transport, config, backend_version)
        .await?)
}

/// `session.fork` (FLOW-FORK-SESSION): a new session from a copy of the
/// conversation of `params.id`, in its workspace and with its backend,
/// model, effort, permission mode and section, not pinned or archived.
pub(super) async fn fork(core: &Core, params: IdParams) -> Result<Session, ApiError> {
    let original = core.sessions.row(&params.id)?;
    let (workspace, project) = ready_workspace(core, &original.workspace_id).await?;
    let Starting {
        transport,
        backend_version,
    } = starting(core, &project, &original.backend).await?;
    let Forkable {
        backend_id,
        adapter,
        beginning,
    } = core
        .sessions
        .prepare_fork(&params.id, Arc::clone(&transport))
        .await?;
    let trusted = trusted_now(core, &project).await?;
    let permission_mode = original
        .permission_mode
        .clone()
        .unwrap_or_else(|| BackendChoices::default_of(&adapter.choices().permission_modes));
    // Chosen and stored before the backend starts, as for a new session, so
    // a fork can be renamed before its first message. A backend without
    // host-chosen ids names it when it starts instead (R28).
    let backend_session_id = adapter.new_session_id();
    let fork = SessionRow {
        id: persistence::new_id("s"),
        workspace_id: workspace.id,
        host_id: project.host_id,
        backend: original.backend,
        backend_session_id: backend_session_id.clone(),
        mode: original.mode,
        // The stored path, never the one the backend reports (R28).
        cwd: workspace.path.clone(),
        model: original.model.clone(),
        effort: original.effort.clone(),
        permission_mode: original.permission_mode,
        archived: false,
        deleting: None,
        pinned: false,
        section_id: original.section_id,
        last_activity_at: None,
    };
    let config = SessionConfig {
        cwd: workspace.path,
        permission_mode,
        model: original.model,
        effort: original.effort,
        project_trusted: trusted,
        project_path: project.path,
        backend_session_id,
        fork_of: Some(backend_id),
        extra: Value::Null,
    };
    Ok(core
        .sessions
        .create_with(fork, beginning, adapter, transport, config, backend_version)
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

/// `session.arrange`: changes the arrangement fields given and leaves the
/// others (FLOW-ORGANIZE-SESSIONS).
pub(super) async fn arrange(core: &Core, params: ArrangeParams) -> Result<Session, ApiError> {
    Ok(core
        .sessions
        .arrange(&params.id, params.pinned, params.section_id)
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

/// A workspace folder's resolution, once it is done: its canonical path, or
/// why it can't be read.
type Resolution = Option<Result<std::path::PathBuf, ()>>;

/// The workspace folders being resolved for `attachment.inspect`, by path.
/// Each folder is resolved by one thread at a time, which every inspect of
/// that workspace waits on, so a workspace on a drive that stops answering
/// holds one thread, never an inspect's place nor another workspace's answer.
static RESOLVING: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, tokio::sync::watch::Receiver<Resolution>>>,
> = std::sync::LazyLock::new(Default::default);

/// How long `attachment.inspect` waits for a workspace folder to resolve,
/// past which every file and folder is taken to be copied.
const RESOLVE_LIMIT: Duration = Duration::from_secs(5);

/// The workspace folder as send resolves it (`Err` when it can't be read),
/// or `None` when it does not answer in time.
async fn resolve_workspace(path: String) -> Option<Result<std::path::PathBuf, ()>> {
    let mut resolution = {
        let mut resolving = RESOLVING.lock().expect("resolving lock");
        match resolving.get(&path) {
            // Already being resolved: wait on that one.
            Some(rx) => rx.clone(),
            None => {
                let (tx, rx) = tokio::sync::watch::channel(None);
                resolving.insert(path.clone(), rx.clone());
                tokio::task::spawn_blocking(move || {
                    let resolved = std::fs::canonicalize(&path).map_err(|_| ());
                    let _ = tx.send(Some(resolved));
                    RESOLVING.lock().expect("resolving lock").remove(&path);
                });
                rx
            }
        }
    };
    let done = tokio::time::timeout(RESOLVE_LIMIT, resolution.wait_for(Option::is_some)).await;
    match done {
        Ok(Ok(resolved)) => resolved.clone(),
        _ => None,
    }
}

/// `attachment.inspect` (PROTO-CORE-API-R24, R44), on the local host: the
/// only one Core reaches yet. With a workspace, it also answers whether
/// sending would copy the path, by send's own decision.
pub(super) async fn inspect(core: &Core, params: InspectParams) -> Result<Inspected, ApiError> {
    // A workspace reads the path on its own host.
    let (host, workspace) = match (params.workspace_id, params.host_id) {
        (Some(id), None) => {
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
            (project.host_id, Some(workspace.path))
        }
        (None, Some(host)) => (host, None),
        _ => {
            return Err(ApiError::invalid_params(
                "name either the workspace or the host, not both",
            ));
        }
    };
    if host != LOCAL_HOST_ID {
        return Err(ApiError::not_found(format!("no host {host}")));
    }
    // Resolved before a place is taken. `Some(None)`: a workspace that did
    // not resolve in time, whose files and folders are taken to be copied.
    let workspace = match workspace {
        Some(path) => Some(resolve_workspace(path).await),
        None => None,
    };
    let images = core.sessions.image_types();
    let path = params.path;
    let place = INSPECTING.try_acquire().map_err(|_| {
        ApiError::invalid_params(
            "too many paths asked about are not answering; try again once they do",
        )
    })?;
    // A blocking thread: a network drive that no longer answers holds that
    // thread, never the request, which is answered at the limit. The path is
    // read and answered first, then whether sending copies it is judged, so a
    // judgment that runs long never fails a path that was read; one deadline
    // covers both.
    let deadline = tokio::time::Instant::now() + INSPECT_LIMIT;
    let (read_tx, read_rx) = tokio::sync::oneshot::channel();
    let (judged_tx, judged_rx) = tokio::sync::oneshot::channel();
    let judging = match &workspace {
        Some(Some(Ok(resolved))) => Some(resolved.clone()),
        _ => None,
    };
    let judged_path = path.clone();
    tokio::task::spawn_blocking(move || {
        let _place = place;
        let read = attachments::inspect(&path, &images);
        let kind = match &read {
            Ok(i) if i.kind != AttachmentKind::Image => Some(i.kind),
            _ => None,
        };
        let _ = read_tx.send(read);
        if let (Some(kind), Some(workspace)) = (kind, judging) {
            let folder = kind == AttachmentKind::Folder;
            let _ = judged_tx.send(attachments::copies(&judged_path, folder, &workspace));
        }
    });
    let mut inspected = match tokio::time::timeout_at(deadline, read_rx).await {
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
    // An image goes inline, never copied; given a host, as for a new workspace
    // not made yet, every file and folder is. Where the workspace itself can't
    // be read, or did not resolve in time, or the judgment is not done by the
    // deadline, it is taken to be a copy, so the note never says one won't
    // happen when it may; the send judges it again. A folder holding the
    // workspace is refused (PROTO-CORE-API-R44).
    inspected.copied = match (&inspected.kind, &workspace) {
        (AttachmentKind::Image, _) => false,
        (_, None) | (_, Some(None)) | (_, Some(Some(Err(_)))) => true,
        (_, Some(Some(Ok(_)))) => match tokio::time::timeout_at(deadline, judged_rx).await {
            Ok(Ok(Ok(copied))) => copied,
            Ok(Ok(Err(reason))) => {
                return Err(ApiError::new(
                    ersa_protocol::core_api::ErrorKind::AttachmentRefused,
                    reason,
                ));
            }
            _ => true,
        },
    };
    Ok(inspected)
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
    find(core, &params.id).await?;
    // Declining says the user gave no answers, so it carries none.
    let reply = match (params.declined.unwrap_or(false), params.answers.is_empty()) {
        (true, true) => QuestionReply::Declined,
        (true, false) => {
            return Err(ApiError::invalid_params(
                "a declined question carries no answers",
            ));
        }
        (false, _) => QuestionReply::Answers(params.answers),
    };
    core.sessions
        .answer_question(&params.id, &params.request_id, reply)
        .await?;
    find(core, &params.id).await
}

/// `text` as one line: each run of spacing (a line break and a tab
/// included) as one space, none at the ends. `None` when it holds any other
/// control character. A session's title and a section's name both take
/// this form.
pub(super) fn one_line(text: &str) -> Option<String> {
    if text.chars().any(|c| c.is_control() && !c.is_whitespace()) {
        return None;
    }
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
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
    let Some(title) = one_line(&params.title) else {
        return Err(ApiError::invalid_params(
            "a session's title is one line, without control characters",
        ));
    };
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
