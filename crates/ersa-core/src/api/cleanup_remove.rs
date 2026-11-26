//! `cleanup.remove` (FLOW-CLEAN-UP-R13, R14, R27): removes one Clean up
//! item the user approved, and nothing they were not shown.
//!
//! Each kind goes the way Core already removes such a thing:
//!
//! - a `workspace` item is put away (`put_away`): its folder and git's
//!   record go, the workspace, its sessions and every branch stay;
//! - a `leftover` item is removed as the sweep removes one
//!   (`handlers::remove_workspace`): its folder, git's record, its branch
//!   and the workspace;
//! - a `branch` item's branch is deleted;
//! - a `folder` item's folder is deleted.
//!
//! The guard is the token: the item is judged again where nothing of
//! Ersa's can change it (the workspace held by admission, in the project's
//! turn), and when its token is not the one the user was shown, nothing is
//! removed and the answer says what the item is now. Then what removing it
//! would lose decides whether `discard` is needed.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ersa_protocol::core_api::{
    CleanupInspection, CleanupKind, CleanupRemoveParams, CleanupRemoveResult, ErrorReason,
    IdParams, WorkspaceKind,
};

use super::cleanup::{self, BRANCH_ID, FOLDER_ID, WORKSPACE_ID};
use super::handlers::{self, Remover, RepositoryTurns};
use super::{ApiError, put_away};
use crate::admission::Refusal;
use crate::platform;
use crate::settings;
use crate::state::Core;
use crate::workspace::{self, Worktrees as _};

/// `cleanup.remove`.
pub(super) async fn remove(
    core: &Core,
    params: CleanupRemoveParams,
) -> Result<CleanupRemoveResult, ApiError> {
    let CleanupRemoveParams { id, token, discard } = params;
    let discard = discard.unwrap_or(false);
    let branch_kept = if let Some(workspace) = id.strip_prefix(WORKSPACE_ID) {
        workspace_item(core, &id, workspace, token, discard).await?
    } else if let Some(rest) = id.strip_prefix(BRANCH_ID) {
        branch_item(core, &id, rest, &token, discard).await?;
        None
    } else if id.starts_with(FOLDER_ID) {
        folder_item(core, &id, &token, discard).await?;
        None
    } else {
        return Err(gone(&id));
    };
    Ok(CleanupRemoveResult { id, branch_kept })
}

/// What the user approved removing: the item as they were shown it.
pub(super) struct Approval {
    id: String,
    kind: CleanupKind,
    token: String,
    /// They approved losing what the item holds.
    pub discard: bool,
    /// Set when the item turned out to have changed since it was shown, so
    /// the answer, made once every hold is let go, says what it is now.
    changed: AtomicBool,
}

impl Approval {
    /// Checks the item against what the user was shown, judged from
    /// `workspace` as it was before the removal held it. Called in the
    /// project's turn, holding one of the places R116 counts as an inspect
    /// does, just before anything is stored or removed.
    pub(super) async fn check(
        &self,
        core: &Core,
        workspace: &ersa_protocol::core_api::Workspace,
    ) -> Result<(), ApiError> {
        let judged = workspace::in_one_place(cleanup::judge_held(core, workspace)).await??;
        let Some(now) = judged else {
            return Err(gone(&self.id));
        };
        // The token covers the kind too; a kind that is not the one this
        // removal is for is a change all the same.
        if now.kind != self.kind || now.token != self.token {
            self.changed.store(true, Ordering::SeqCst);
            return Err(ApiError::invalid_state(
                "the item changed since it was inspected",
            ));
        }
        if !self.discard && loses_unsaved_work(&now) {
            return Err(unsaved(&self.id));
        }
        Ok(())
    }

    /// The answer for a refusal by admission of this item's removal.
    pub(super) fn refused(&self, refusal: Refusal, named: &str) -> ApiError {
        match refusal {
            Refusal::SessionInUse => session_busy(named),
            // What the item is has changed: a session uses a leftover now,
            // or the workspace is no longer `ready`.
            Refusal::WorkspaceHasSessions | Refusal::WorkspaceNotReady => {
                self.changed.store(true, Ordering::SeqCst);
                ApiError::invalid_state("the item changed since it was inspected")
            }
            other => handlers::refused(other, named),
        }
    }

    fn changed(&self) -> bool {
        self.changed.load(Ordering::SeqCst)
    }
}

/// A `workspace` or `leftover` item: the workspace `workspace_id`.
/// Answers the branch kept.
async fn workspace_item(
    core: &Core,
    id: &str,
    workspace_id: &str,
    token: String,
    discard: bool,
) -> Result<Option<String>, ApiError> {
    // Found first, as the list finds them, so admission sees them.
    cleanup::find_elsewhere(core).await?;
    let workspace = core
        .store
        .workspace(workspace_id.to_string())
        .await?
        .ok_or_else(|| gone(id))?;
    // The project's own folder is never an item (FLOW-CLEAN-UP-R14).
    if workspace.kind == WorkspaceKind::Root {
        return Err(gone(id));
    }
    // Before anything else, since a workspace with a session running is no
    // item at all and would read as gone.
    if core.admission.session_in_use(workspace_id) {
        return Err(session_busy(workspace_id));
    }
    // Which removal it takes: a workspace a session refers to is put away,
    // one none refers to is a leftover. Both are judged again in the turn,
    // where a kind that changed meanwhile is refused.
    let used = core
        .store
        .sessions()
        .await?
        .iter()
        .any(|row| row.workspace_id == workspace_id);
    let approval = Approval {
        id: id.to_string(),
        kind: if used {
            CleanupKind::Workspace
        } else {
            CleanupKind::Leftover
        },
        token,
        discard,
        changed: AtomicBool::new(false),
    };
    let removed = if used {
        // Put away keeps every branch.
        put_away::put_away_approved(core, workspace_id, &approval)
            .await
            .map(|()| true)
    } else {
        handlers::remove_workspace(core, &workspace, Remover::Cleanup(&approval)).await
    };
    let kept = match removed {
        Ok(kept) => kept,
        Err(_) if approval.changed() => return Err(changed(core, id, workspace_id).await),
        Err(e) => return Err(e),
    };
    let branch = &workspace.vcs_ref;
    tracing::info!(
        item = %id,
        path = %workspace.path,
        kind = ?approval.kind,
        discard,
        "cleanup.remove removed a workspace's folder, as the user approved"
    );
    if !kept || branch.is_empty() {
        return Ok(None);
    }
    // Kept, unless it was gone already: deleted by hand.
    let project = handlers::find_project(core, &workspace.project_id).await?;
    let transport = cleanup::transport(core, &project)?;
    let exists = workspace::git_branch_exists(transport.as_ref(), &project.path, branch).await?;
    Ok(exists.then(|| branch.clone()))
}

/// A `branch` item: `rest` is `<project id>:<branch>`. Deleted in the
/// project's turn, so no worktree of Ersa's checks it out meanwhile.
async fn branch_item(
    core: &Core,
    id: &str,
    rest: &str,
    token: &str,
    discard: bool,
) -> Result<(), ApiError> {
    let (project_id, branch) = rest.split_once(':').ok_or_else(|| gone(id))?;
    let project = core
        .store
        .projects()
        .await?
        .into_iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| gone(id))?;
    let turns = RepositoryTurns {
        core,
        project: &project.id,
    };
    let turn = turns.turn().await?;
    // Judged again in the turn: still an `ersa/…` branch nothing uses
    // (FLOW-CLEAN-UP-R14), and what removing it loses as it was shown.
    let now = workspace::in_one_place(cleanup::inspect_held(core, id)).await??;
    if now.token != token {
        drop(turn);
        return Err(changed_to(&now));
    }
    if !discard && loses_unsaved_work(&now) {
        return Err(unsaved(id));
    }
    let transport = cleanup::transport(core, &project)?;
    workspace::git_delete_branch(transport.as_ref(), &project.path, branch).await?;
    drop(turn);
    tracing::info!(item = %id, branch, discard, "cleanup.remove deleted a branch, as the user approved");
    Ok(())
}

/// A `folder` item. Judged again in the turns of every project whose
/// workspaces go in the same folder under the worktrees root, so none adds a
/// worktree there meanwhile; then deleted with the tries every removal of a
/// folder makes, the turns let go, since no worktree is in it.
async fn folder_item(core: &Core, id: &str, token: &str, discard: bool) -> Result<(), ApiError> {
    let turns = sharing_turns(core, id).await?;
    let now = workspace::in_one_place(cleanup::inspect_held(core, id)).await??;
    // A folder whose measuring did not finish has a token that matches
    // none (FLOW-CLEAN-UP-R12), so it is refused here, `discard` or not.
    if now.token != token {
        drop(turns);
        return Err(changed_to(&now));
    }
    if !discard {
        return Err(unsaved(id));
    }
    drop(turns);
    let path = now.path.ok_or_else(|| gone(id))?;
    let tries = workspace::FolderTries::from_config(&core.config, Arc::new(()));
    workspace::remove_folder(&path, &tries).await?;
    tracing::info!(item = %id, path, "cleanup.remove deleted a folder, as the user approved");
    Ok(())
}

/// The turns of every project whose workspaces go in the same folder under
/// the worktrees root as the folder item `id`'s project, taken in the order
/// of their ids, as the sweep takes them to tidy that folder.
async fn sharing_turns(core: &Core, id: &str) -> Result<Vec<Box<dyn Send>>, ApiError> {
    let rest = id.strip_prefix(FOLDER_ID).ok_or_else(|| gone(id))?;
    let (project_id, _) = rest.split_once(':').ok_or_else(|| gone(id))?;
    let projects = core.store.projects().await?;
    let project = projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| gone(id))?;
    let root = settings::workspaces_root(&core.store.settings().await?, &core.config.data_dir);
    // Compared as the file system compares names: without case on Windows.
    let key = |name: &str| platform::path_key(&root.join(name).display().to_string());
    let mut sharing: Vec<&str> = projects
        .iter()
        .filter(|other| key(&other.name) == key(&project.name))
        .map(|other| other.id.as_str())
        .collect();
    sharing.sort();
    let mut turns = Vec::new();
    for project in sharing {
        let asked = RepositoryTurns { core, project };
        turns.push(asked.turn().await?);
    }
    Ok(turns)
}

/// Whether removing the item as `inspected` loses unsaved work (ADR 0038):
/// what only its folder holds, uncommitted files and commits its branch
/// does not hold, or what can't be counted; a repository of its own in the
/// folder (FLOW-CLEAN-UP-R27); a branch's commits no other branch holds,
/// merged or not, since those commits themselves go. A leftover's own
/// branch holding such commits is kept without `discard`, so they are not
/// lost; ignored files are no unsaved work. A folder item is refused
/// without `discard` whatever it holds, by its caller.
fn loses_unsaved_work(inspected: &CleanupInspection) -> bool {
    let counted = |n: Option<u64>| n.unwrap_or(0) > 0;
    match inspected.kind {
        CleanupKind::Workspace | CleanupKind::Leftover => {
            counted(inspected.uncommitted)
                || counted(inspected.stranded)
                || inspected.unrecorded.unwrap_or(false)
                || inspected.holds_repository.unwrap_or(false)
        }
        CleanupKind::Branch => counted(inspected.unshared),
        CleanupKind::Folder => true,
    }
}

/// The answer for an item that changed since it was shown: `invalid_state`
/// with the item as it is now, unless a session there is running or
/// starting now, or the item is gone.
async fn changed(core: &Core, id: &str, workspace_id: &str) -> ApiError {
    if core.admission.session_in_use(workspace_id) {
        return session_busy(workspace_id);
    }
    match cleanup::inspect(core, IdParams { id: id.to_string() }).await {
        Ok(now) => changed_to(&now),
        Err(e) => e,
    }
}

/// `invalid_state` whose `detail` is `now`, the item as `cleanup.inspect`
/// gives it, as JSON text.
fn changed_to(now: &CleanupInspection) -> ApiError {
    let detail = serde_json::to_string(now).unwrap_or_default();
    ApiError::invalid_state(format!(
        "{} changed since it was inspected; nothing was removed",
        now.id
    ))
    .with_detail(detail)
}

fn unsaved(id: &str) -> ApiError {
    ApiError::invalid_state(format!(
        "removing {id} would lose work that exists nowhere else, or it is a folder Ersa \
         can't identify; nothing was removed, and it goes only with discard"
    ))
}

fn session_busy(workspace_id: &str) -> ApiError {
    ApiError::invalid_state(format!(
        "a session in workspace {workspace_id} is running or starting; nothing was removed"
    ))
    .with_reason(ErrorReason::SessionBusy)
}

fn gone(id: &str) -> ApiError {
    ApiError::not_found(format!("no Clean up item {id}"))
}
