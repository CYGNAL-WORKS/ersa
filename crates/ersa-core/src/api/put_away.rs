//! Putting an isolated workspace away (FLOW-CLEAN-UP, Putting away and
//! putting back): its folder and git's record of the worktree go, and the
//! workspace, its sessions and every branch stay, with what the folder had
//! checked out stored so the folder can come back as it was.
//!
//! The sweep puts a workspace away once it has checked that the workspace
//! holds no unsaved work (FLOW-CLEAN-UP-R22), and `cleanup.remove` once the
//! user approved it (FLOW-CLEAN-UP-R13).

use std::sync::Arc;

use ersa_protocol::core_api::{Workspace, WorkspaceState};

use super::cleanup_remove::Approval;
use super::{ApiError, handlers};
use crate::admission::{Command, Report, end_held};
use crate::state::Core;
use crate::workspace;

/// Puts workspace `id` away (FLOW-CLEAN-UP-R02).
///
/// 1. Admission lets it begin only while nothing is under way on the
///    workspace or a session there; it then holds the workspace, so nothing
///    starts there and nothing else removes it.
/// 2. It waits its turn at the project's worktrees (PROTO-CORE-API-R124).
/// 3. What the folder has checked out is read: a branch, or the commit when
///    it is on none. Read in the turn, so it is what the folder has when
///    its removal begins.
/// 4. A folder that holds a repository of its own is refused, in the turn
///    (PROTO-CORE-API-R147).
/// 5. That and the state `put_away` are stored together, before anything
///    is removed, so however Core stops from here the workspace is put away
///    with its checkout known.
/// 6. The folder goes as every removal takes one (PROTO-CORE-API-R122),
///    starting in the turn already held, keeping every branch.
///
/// A refusal before anything was removed (a repository of its own, no turn
/// in time, a checkout that can't be read, a lock, local changes, a folder
/// git does not list) leaves the workspace `ready`, as it was
/// (PROTO-CORE-API-R140, R147): put
/// away, its whole folder would go unchecked when it is put back. Any
/// other failure leaves it `put_away` (R141), what is left of the folder
/// going when it is put back (FLOW-CLEAN-UP-R03).
///
/// It does not ask whether the workspace's sessions are archived: the Clean
/// up list also puts away the folders of sessions that are only inactive
/// (FLOW-CLEAN-UP-R11, R13), so whoever calls it decides which sessions may
/// be there. The sweep calls [`put_away_for_sweep`], never this.
pub async fn put_away_workspace(core: &Core, id: &str) -> Result<(), ApiError> {
    put_away(core, id, false, None).await
}

/// [`put_away_workspace`] for the sweep: refused also unless a session uses
/// the workspace and every one there is archived, which admission decides
/// as it admits the put-away (FLOW-CLEAN-UP-R22).
pub(super) async fn put_away_for_sweep(core: &Core, id: &str) -> Result<(), ApiError> {
    put_away(core, id, true, None).await
}

/// [`put_away_workspace`] for `cleanup.remove` of a `workspace` item
/// (FLOW-CLEAN-UP-R13): checked again in the project's turn against what
/// the user was shown, before anything is stored. With `discard` it is
/// forced, so uncommitted files go with the folder, and a folder that holds
/// a repository of its own goes too (FLOW-CLEAN-UP-R27, PROTO-CORE-API-R147).
pub(super) async fn put_away_approved(
    core: &Core,
    id: &str,
    approval: &Approval,
) -> Result<(), ApiError> {
    put_away(core, id, false, Some(approval)).await
}

async fn put_away(
    core: &Core,
    id: &str,
    archived_only: bool,
    approval: Option<&Approval>,
) -> Result<(), ApiError> {
    let workspace = core
        .store
        .workspace(id.to_string())
        .await?
        .ok_or_else(|| ApiError::not_found(format!("no workspace {id}")))?;
    let project = handlers::find_project(core, &workspace.project_id).await?;
    let transport = core
        .transport_for(&project.host_id)
        .ok_or_else(|| handlers::unreachable_host(&project.host_id))?;

    // (1) Shared with each try at the folder, which keeps it until its
    // thread returns: a try given up on that is still stuck holds the
    // workspace, so no other removal of it begins (PROTO-CORE-API-R131).
    let putting_away = core
        .admission
        .command(Command::PutAway {
            workspace: id.to_string(),
            archived_only,
        })
        .await
        .map_err(|refusal| match approval {
            Some(approval) => approval.refused(refusal, id),
            None => handlers::refused(refusal, id),
        })?;
    let refused = || Report::PutAwayRefused {
        workspace: id.to_string(),
    };
    let put_away = || Report::WorkspacePutAway {
        workspace: id.to_string(),
    };

    // (2) Past the wait, no git work begins and nothing was stored.
    let turns = handlers::RepositoryTurns {
        core,
        project: &project.id,
    };
    let turn = match workspace::Worktrees::turn(&turns).await {
        Ok(turn) => turn,
        Err(e) => {
            putting_away.end(refused());
            return Err(e.into());
        }
    };
    // (2b) What the user approved is what is there now: judged from the
    // workspace as it was before this put-away held it, `ready`.
    let as_it_was = Workspace {
        state: WorkspaceState::Ready,
        ..workspace.clone()
    };
    if let Some(approval) = approval
        && let Err(e) = approval.check(core, &as_it_was).await
    {
        putting_away.end(refused());
        return Err(e);
    }
    let discard = approval.is_some_and(|approval| approval.discard);
    // (3) A read, done in the turn.
    let checkout = match workspace::git_checkout(transport.as_ref(), &workspace.path).await {
        Ok(checkout) => checkout,
        Err(e) => {
            putting_away.end(refused());
            return Err(e.into());
        }
    };
    // (4) A folder that holds a repository of its own stays
    // (PROTO-CORE-API-R147): git would refuse it only once the workspace
    // was stored `put_away`, leaving the whole folder for a put-back to
    // remove unchecked. Looked at in the turn, after any wait for it, so
    // nothing checked out meanwhile is missed; a check that fails keeps the
    // folder too. Only a `cleanup.remove` with `discard` takes it.
    let holds = if discard {
        Ok(false)
    } else {
        workspace::git_holds_repository(transport.as_ref(), &project.path, &workspace.path).await
    };
    match holds {
        Ok(false) => {}
        Ok(true) => {
            putting_away.end(refused());
            return Err(workspace::VcsError::HoldsRepository.into());
        }
        Err(e) => {
            putting_away.end(refused());
            return Err(e.into());
        }
    }
    // (5) Before anything is removed.
    if let Err(e) = core
        .store
        .put_workspace_away(id.to_string(), checkout)
        .await
    {
        putting_away.end(refused());
        return Err(e.into());
    }

    // (6) The folder and git's record; no branch is deleted, since the
    // branch is how the folder comes back. Not forced: whoever puts away has
    // checked that nothing in the folder is unsaved, and a change since then
    // refuses the removal rather than lose it; unless the user approved
    // discarding what it holds.
    let putting_away = Arc::new(putting_away);
    let turns = workspace::HeldFirst::new(turn, turns);
    let removed = workspace::git_worktree_delete(
        transport.as_ref(),
        &project.path,
        &workspace.path,
        "",
        workspace::RemovalChecks {
            force: discard,
            after_failed_add: false,
            discard_branch: false,
        },
        workspace::FolderTries::from_config(&core.config, Arc::clone(&putting_away) as _),
        &turns,
    )
    .await;
    let failed = match removed {
        Ok(_) => {
            end_held(putting_away, put_away());
            return Ok(());
        }
        Err(e) => e,
    };
    if failed.is_refusal() {
        tracing::warn!(workspace = %id, error = %failed, "the workspace was not put away");
        // Stored, then said, as every way out of an operation is. Not
        // stored, it stays put away, as the store has it.
        match core.store.set_workspace_ready(id.to_string()).await {
            Ok(()) => end_held(putting_away, refused()),
            Err(e) => {
                tracing::warn!(workspace = %id, error = %e, "could not put the workspace back to ready");
                end_held(putting_away, put_away());
            }
        }
    } else {
        tracing::warn!(workspace = %id, error = %failed, "putting the workspace away failed part way");
        end_held(putting_away, put_away());
    }
    Err(failed.into())
}
