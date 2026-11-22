//! `session.delete`, the user's Delete (PROTO-CORE-API, Sessions;
//! FLOW-ARCHIVE-DELETE-SESSION): stops the session, deletes its isolated
//! workspace's folder and branch when nothing else needs them, deletes its
//! record from its backend, and removes it.
//!
//! The steps, in the brief's order:
//!
//! 1. Hold the workspace, in admission: requests that would change the
//!    session or its workspace are refused `busy` until the delete ends.
//! 2. Stop the session, then ask the backend's registry whether another
//!    process has it open. Core cannot tell its own process in the registry
//!    from another one, so its own is stopped first; a CLI takes itself out
//!    of the registry as it exits.
//! 3. For an isolated workspace, compute `workspace.changes` again and
//!    refuse when its token is not the one the user was shown.
//! 4. Mark the session deleting, recording whether the files go.
//! 5. Delete the folder and, unless it holds work of its own, the branch.
//! 6. Delete the session's record from its backend.
//! 7. Remove the session, and the workspace when its files went.
//!
//! Up to step 3, a refusal deletes and marks nothing. From step 4 on, a
//! failure marks the delete failed, and a new `session.delete` with a fresh
//! token finishes it.

use std::sync::Arc;

use tracing::Instrument;

use ersa_adapter::{OpenElsewhere, RecordDeletion};
use ersa_protocol::core_api::{
    ErrorReason, SessionDeleteParams, SessionDeleteResult, Workspace, WorkspaceKind,
};

use super::{ApiError, handlers};
use crate::admission::UnderWay;
use crate::sessions::SessionError;
use crate::state::Core;
use crate::workspace;

pub(super) async fn delete(
    core: &Core,
    params: SessionDeleteParams,
) -> Result<SessionDeleteResult, ApiError> {
    let id = params.id;
    let row = core.sessions.row(&id)?;
    let workspace = core
        .store
        .workspace(row.workspace_id.clone())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no workspace {}", row.workspace_id)))?;
    let isolated = workspace.kind == WorkspaceKind::Isolated;
    // A root workspace's files are never touched, so it needs no token.
    let token = match (isolated, params.token) {
        (true, Some(token)) => token,
        (true, None) => {
            return Err(ApiError::invalid_params(
                "a session in an isolated workspace is deleted with the token of the \
                 workspace.changes the user was shown",
            ));
        }
        (false, _) => String::new(),
    };
    let session_transport = core
        .session_transport_for(&row.host_id)
        .ok_or_else(|| handlers::unreachable_host(&row.host_id))?;

    // (1) Held to the end of this function, and by each call of steps 5
    // and 6 until that call has ended, however long after this request
    // gave up on it: the last holder to let go ends the delete in
    // admission and releases the workspace. So a try again while a call
    // Core gave up on is still stuck is refused `busy`, and never begins a
    // second one beside it.
    let deleting = Arc::new(core.sessions.hold_for_delete(&id).await?);
    // Refused while it starts or works. Checked once the workspace is held,
    // so a send that arrives after it is refused `busy` and one that
    // arrived before is seen here; the refusal lets go, having changed
    // nothing.
    core.sessions.check_deletable(&id)?;

    // (2) A stop that fails or times out ends the delete with its error.
    core.sessions.stop(&id).await?;
    match core
        .sessions
        .open_elsewhere(&id, session_transport.clone())
        .await?
    {
        OpenElsewhere::No => {}
        refused => return Err(open_elsewhere(&id, refused)),
    }

    // (3) Only what the user was shown goes.
    let mut files = false;
    if isolated {
        let (changes, counted) = handlers::changes_of(core, &workspace).await?;
        if changes.token != token {
            let detail = serde_json::to_string(&changes)
                .map_err(|e| ApiError::internal(format!("serializing: {e}")))?;
            return Err(ApiError::invalid_state(
                "what deleting the workspace would lose has changed since the user was asked",
            )
            .with_detail(detail));
        }
        // The files go only when no other session uses the workspace.
        files = counted.iter().all(|counted| counted == &id);
    }

    // (4) What is recorded wins: a delete that once kept the files keeps them.
    // When the files go, the workspace leaves `ready` in the same
    // transaction, so no new session starts in a folder on its way out,
    // whatever step fails later or however Core stops (`workspace.remove`
    // takes it from `error`, and a finished delete removes its row).
    let files = core.sessions.mark_deleting(&id, files).await?;

    // (5) to (7). A failure from here on leaves the session marked failed.
    let held = Arc::clone(&deleting);
    match finish(core, &id, &workspace, files, session_transport, held).await {
        Ok(result) => Ok(result),
        Err(e) => {
            if let Err(mark) = core.sessions.mark_delete_failed(&id).await {
                tracing::error!(session = %id, error = %mark, "could not mark a delete failed");
            }
            Err(e)
        }
    }
}

/// Steps 5 to 7: the files, the record, and the rows. `held` is the
/// delete's claim, which each call of steps 5 and 6 keeps until it ends.
async fn finish(
    core: &Core,
    id: &str,
    workspace: &Workspace,
    files: bool,
    session_transport: Arc<dyn ersa_adapter::Transport>,
    held: Arc<UnderWay>,
) -> Result<SessionDeleteResult, ApiError> {
    let mut branch_kept = false;
    if files {
        // In a span naming the session and step, so the warning git's
        // failed removal writes to the diagnostic log says which delete it
        // was (PROTO-CORE-API-R114).
        let step = tracing::warn_span!("session_delete", session = %id, step = 5);
        let deleted = delete_files(core, workspace, &held).instrument(step).await;
        branch_kept = deleted.inspect_err(|e| step_failed(id, 5, e))?;
    }
    let limit = core.config.record_deletion_limit;
    let deleted = match core
        .sessions
        .delete_record(id, session_transport, limit, held)
        .await
    {
        Ok(RecordDeletion::Deleted) => Ok(()),
        // Opened elsewhere since step 2.
        Ok(RecordDeletion::Refused(refused)) => Err(open_elsewhere(id, refused)),
        Err(SessionError::Adapter(e)) => Err(ApiError::internal(format!(
            "deleting the session's record failed: {e}"
        ))),
        Err(e) => Err(e.into()),
    };
    deleted.inspect_err(|e| step_failed(id, 6, e))?;
    // The workspace's row goes when its files went, or when an earlier
    // delete of another session in it took them (this one's record kept
    // them, since it shared the workspace then); in either case only once
    // no session refers to it.
    let isolated = workspace.kind == WorkspaceKind::Isolated;
    let taken_earlier = isolated && !files && files_taken(core, workspace).await;
    core.sessions
        .finish_delete(
            id,
            (isolated && (files || taken_earlier)).then(|| workspace.id.clone()),
        )
        .await?;
    Ok(SessionDeleteResult {
        id: id.to_string(),
        workspace_deleted: files,
        branch_kept,
    })
}

/// Step 5: the workspace's folder and, unless it holds work of its own,
/// its branch. Returns whether the branch was kept.
async fn delete_files(
    core: &Core,
    workspace: &Workspace,
    held: &Arc<UnderWay>,
) -> Result<bool, ApiError> {
    let project = handlers::find_project(core, &workspace.project_id).await?;
    let transport = core
        .transport_for(&project.host_id)
        .ok_or_else(|| handlers::unreachable_host(&project.host_id))?;
    let kept = workspace::git_worktree_delete(
        transport.as_ref(),
        &project.path,
        &workspace.path,
        &workspace.vcs_ref,
        // Forced: the user was shown what the workspace held.
        workspace::RemovalChecks::FORCED,
        workspace::FolderTries::from_config(&core.config, Arc::clone(held) as _),
        &handlers::RepositoryTurns {
            core,
            project: &project.id,
        },
    )
    .await?;
    Ok(kept)
}

/// Writes step `step`'s failure to the diagnostic log (PROTO-CORE-API-R114):
/// the user's tile says only that the delete failed.
fn step_failed(id: &str, step: u8, error: &ApiError) {
    let detail = error
        .0
        .data
        .as_ref()
        .and_then(|data| data.detail.as_deref());
    tracing::warn!(
        session = %id,
        step,
        reason = %error.0.message,
        detail = detail.unwrap_or_default(),
        "a step of the session's delete failed"
    );
}

/// Whether an isolated workspace's files were deleted already: git no
/// longer lists its worktree, and its folder is gone. A folder missing while
/// git still lists the worktree (a drive unplugged, a folder deleted by
/// hand) was not deleted by Ersa, and anything Core cannot tell counts as
/// still there, so the row is kept.
async fn files_taken(core: &Core, workspace: &Workspace) -> bool {
    let Ok(project) = handlers::find_project(core, &workspace.project_id).await else {
        return false;
    };
    let Some(transport) = core.transport_for(&project.host_id) else {
        return false;
    };
    let gone =
        workspace::git_worktree_gone(transport.as_ref(), &project.path, &workspace.path).await;
    matches!(gone, Ok(true))
}

/// The refusal for a session another process has open, or may have.
fn open_elsewhere(id: &str, answer: OpenElsewhere) -> ApiError {
    match answer {
        OpenElsewhere::Yes { pids } => SessionError::open_elsewhere(
            &pids,
            format!(
                "another process ({}) has session {id} open; close it there, then delete it",
                crate::sessions::pids_named(&pids)
            ),
        )
        .into(),
        OpenElsewhere::Unknown => SessionError::refused(
            ErrorReason::OpenUnknown,
            format!(
                "Ersa could not tell whether another process has session {id} open, \
                 so it deleted nothing"
            ),
        )
        .into(),
        // A backend that refused while saying nothing has the session open
        // broke its contract: not a reason the user can act on.
        OpenElsewhere::No => ApiError::internal(format!(
            "the backend refused to delete session {id}'s record, \
             though it says no process has the session open"
        )),
    }
}
