//! `session.delete`, the user's Delete (PROTO-CORE-API, Sessions;
//! FLOW-ARCHIVE-DELETE-SESSION): stops the session, deletes its isolated
//! workspace's folder and branch when nothing else needs them, deletes its
//! record from its backend, and removes it.
//!
//! The steps, in the brief's order:
//!
//! 1. Claim the workspace, in memory only: requests that would change the
//!    session or its workspace are refused `busy` until the claim goes.
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

use ersa_adapter::{OpenElsewhere, RecordDeletion};
use ersa_protocol::core_api::{
    SessionDeleteParams, SessionDeleteResult, Workspace, WorkspaceKind,
};

use super::{ApiError, handlers};
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

    // (1) Held to the end of this function: every way out releases it.
    let _claim = core.sessions.claim_workspace(&workspace.id, &id)?;
    // Refused while it starts or works. Checked once the claim is held, so
    // a send that arrives after it is refused `busy` and one that arrived
    // before is seen here; the refusal releases the claim, having changed
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
    let files = core.sessions.mark_deleting(&id, files).await?;

    // (5) to (7). A failure from here on leaves the session marked failed.
    match finish(core, &id, &workspace, files, session_transport).await {
        Ok(result) => Ok(result),
        Err(e) => {
            if let Err(mark) = core.sessions.mark_delete_failed(&id).await {
                tracing::error!(session = %id, error = %mark, "could not mark a delete failed");
            }
            Err(e)
        }
    }
}

/// Steps 5 to 7: the files, the record, and the rows.
async fn finish(
    core: &Core,
    id: &str,
    workspace: &Workspace,
    files: bool,
    session_transport: std::sync::Arc<dyn ersa_adapter::Transport>,
) -> Result<SessionDeleteResult, ApiError> {
    let mut branch_kept = false;
    if files {
        let project = handlers::find_project(core, &workspace.project_id).await?;
        let transport = core
            .transport_for(&project.host_id)
            .ok_or_else(|| handlers::unreachable_host(&project.host_id))?;
        branch_kept = workspace::git_worktree_delete(
            transport.as_ref(),
            &project.path,
            &workspace.path,
            &workspace.vcs_ref,
        )
        .await?;
    }
    match core.sessions.delete_record(id, session_transport).await {
        Ok(RecordDeletion::Deleted) => {}
        // Opened elsewhere since step 2.
        Ok(RecordDeletion::Refused(refused)) => return Err(open_elsewhere(id, refused)),
        Err(SessionError::Adapter(e)) => {
            return Err(ApiError::internal(format!(
                "deleting the session's record failed: {e}"
            )));
        }
        Err(e) => return Err(e.into()),
    }
    // The workspace's row goes when its files went, or when an earlier
    // delete of another session in it took them (this one's record kept
    // them, since it shared the workspace then); in either case only once
    // no session refers to it.
    let folder_gone = !files && !folder_exists(&workspace.path).await;
    let isolated = workspace.kind == WorkspaceKind::Isolated;
    core.sessions
        .finish_delete(
            id,
            (isolated && (files || folder_gone)).then(|| workspace.id.clone()),
        )
        .await?;
    Ok(SessionDeleteResult {
        id: id.to_string(),
        workspace_deleted: files,
        branch_kept,
    })
}

/// Whether anything is at `path`; a folder Core may not look into, or that
/// does not answer within five seconds, counts as there, so its row is kept.
async fn folder_exists(path: &str) -> bool {
    let path = path.to_string();
    let look = tokio::task::spawn_blocking(move || {
        std::path::Path::new(&path).try_exists().unwrap_or(true)
    });
    !matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), look).await,
        Ok(Ok(false))
    )
}

/// The refusal for a session another process has open, or may have.
fn open_elsewhere(id: &str, answer: OpenElsewhere) -> ApiError {
    match answer {
        OpenElsewhere::Yes { pid } => ApiError::invalid_state(format!(
            "another process (pid {pid}) has session {id} open; close it there, then delete it"
        )),
        OpenElsewhere::Unknown | OpenElsewhere::No => ApiError::invalid_state(format!(
            "Ersa could not tell whether another process has session {id} open, \
             so it deleted nothing"
        )),
    }
}
