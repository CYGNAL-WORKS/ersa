//! Putting an isolated workspace away (FLOW-CLEAN-UP, Putting away and
//! putting back): its folder and git's record of the worktree go, and the
//! workspace, its sessions and every branch stay, with what the folder had
//! checked out stored so the folder can come back as it was.
//!
//! Nothing in the product puts a workspace away yet: the sweep and
//! `cleanup.remove` will, each checking first that the workspace holds no
//! unsaved work (FLOW-CLEAN-UP-R01).

use std::sync::{Arc, Mutex};

use super::{ApiError, handlers};
use crate::admission::{Command, Report};
use crate::state::Core;
use crate::workspace::{self, VcsError};

/// Puts workspace `id` away (FLOW-CLEAN-UP-R02).
///
/// 1. Admission lets it begin only while nothing is under way on the
///    workspace or a session there; it then holds the workspace, so nothing
///    starts there and nothing else removes it.
/// 2. It waits its turn at the project's worktrees (PROTO-CORE-API-R124).
/// 3. What the folder has checked out is read: a branch, or the commit when
///    it is on none. Read in the turn, so it is what the folder has when
///    its removal begins.
/// 4. That and the state `put_away` are stored together, before anything
///    is removed, so however Core stops from here the workspace is put away
///    with its checkout known.
/// 5. The folder goes as every removal takes one (PROTO-CORE-API-R122),
///    starting in the turn already held, keeping every branch.
///
/// A refusal before anything was removed (no turn in time, a checkout that
/// can't be read, a lock, local changes, a folder git does not list)
/// leaves the workspace `ready`, as it was (PROTO-CORE-API-R140): put
/// away, its whole folder would go unchecked when it is put back. Any
/// other failure leaves it `put_away` (R141), what is left of the folder
/// going when it is put back (FLOW-CLEAN-UP-R03).
pub async fn put_away_workspace(core: &Core, id: &str) -> Result<(), ApiError> {
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
        })
        .await
        .map_err(|refusal| handlers::refused(refusal, id))?;
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
    // (3) A read, done in the turn.
    let checkout = match workspace::git_checkout(transport.as_ref(), &workspace.path).await {
        Ok(checkout) => checkout,
        Err(e) => {
            putting_away.end(refused());
            return Err(e.into());
        }
    };
    // (4) Before anything is removed.
    if let Err(e) = core
        .store
        .put_workspace_away(id.to_string(), checkout)
        .await
    {
        putting_away.end(refused());
        return Err(e.into());
    }

    // (5) The folder and git's record; no branch is deleted, since the
    // branch is how the folder comes back. Not forced: whoever puts away has
    // checked that nothing in the folder is unsaved, and a change since then
    // refuses the removal rather than lose it.
    let putting_away = Arc::new(putting_away);
    let turns = HeldFirst {
        held: Mutex::new(Some(turn)),
        then: turns,
    };
    let removed = workspace::git_worktree_delete(
        transport.as_ref(),
        &project.path,
        &workspace.path,
        "",
        workspace::RemovalChecks {
            force: false,
            after_failed_add: false,
        },
        workspace::FolderTries::from_config(&core.config, Arc::clone(&putting_away) as _),
        &turns,
    )
    .await;
    let failed = match removed {
        Ok(_) => {
            handlers::end_held(putting_away, put_away());
            return Ok(());
        }
        Err(e) => e,
    };
    if failed.is_refusal() {
        tracing::warn!(workspace = %id, error = %failed, "the workspace was not put away");
        // Stored, then said, as every way out of an operation is. Not
        // stored, it stays put away, as the store has it.
        match core.store.set_workspace_ready(id.to_string()).await {
            Ok(()) => handlers::end_held(putting_away, refused()),
            Err(e) => {
                tracing::warn!(workspace = %id, error = %e, "could not put the workspace back to ready");
                handlers::end_held(putting_away, put_away());
            }
        }
    } else {
        tracing::warn!(workspace = %id, error = %failed, "putting the workspace away failed part way");
        handlers::end_held(putting_away, put_away());
    }
    Err(failed.into())
}

/// The project's turns, the first of them one already held: the removal
/// asks for a turn as it begins, and gets the one the put-away took before
/// it read and stored the checkout.
struct HeldFirst<'a> {
    held: Mutex<Option<Box<dyn Send>>>,
    then: handlers::RepositoryTurns<'a>,
}

#[ersa_adapter::async_trait]
impl workspace::Worktrees for HeldFirst<'_> {
    async fn turn(&self) -> Result<Box<dyn Send>, VcsError> {
        let held = self.held.lock().expect("not poisoned").take();
        match held {
            Some(turn) => Ok(turn),
            None => self.then.turn().await,
        }
    }
}
