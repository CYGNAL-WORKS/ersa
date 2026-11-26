//! Leftover workspaces (FLOW-CLEAN-UP-R23): isolated workspaces no session
//! refers to, made at least an hour ago, which a start that was cut off, a
//! fork whose cleanup failed or a crash left behind. The sweep removes each
//! one that holds nothing, with its folder, git's record and its branch, and
//! keeps for the Clean up list any that holds something, or that it could
//! not check (R08). It asks nothing and tells no client.

use std::collections::HashSet;

use ersa_protocol::core_api::{Project, Workspace, WorkspaceState};

use super::handlers::{self, Remover, RepositoryTurns};
use crate::platform;
use crate::settings;
use crate::state::Core;
use crate::workspace::{self, Worktrees as _};

/// How long ago a workspace must have been made to be a leftover: long
/// enough to outlast the moment between making a workspace and starting a
/// session there (FLOW-CLEAN-UP-R23).
const LEFTOVER_AGE: time::Duration = time::Duration::hours(1);

/// Why the sweep leaves a workspace no session refers to.
pub(super) enum Keep {
    /// It is no leftover: what a put-away that failed part way left
    /// (FLOW-CLEAN-UP-R23, R24), which no sweep touches.
    NoLeftover(&'static str),
    /// A leftover that holds something, or that could not be checked
    /// (FLOW-CLEAN-UP-R08).
    Holds(String),
}

impl Keep {
    pub(super) fn why(&self) -> String {
        match self {
            Keep::NoLeftover(why) => (*why).to_string(),
            Keep::Holds(why) => why.clone(),
        }
    }
}

/// Why `workspace`, which no session refers to, must stay, if it must
/// (FLOW-CLEAN-UP-R23): what a failed put-away left at its path; anything
/// `workspace.changes` counts as uncommitted, ignored or unshared, or git
/// having no record of it; a repository of its own in its folder
/// (PROTO-CORE-API-R147); or a check that failed. Its age is the sweep's to
/// judge, not this.
pub(super) async fn keeps(core: &Core, workspace: &Workspace) -> Option<Keep> {
    let project = match handlers::find_project(core, &workspace.project_id).await {
        Ok(project) => project,
        Err(e) => {
            return Some(Keep::Holds(format!(
                "its project can't be read: {}",
                e.0.message
            )));
        }
    };
    let Some(transport) = core.transport_for(&project.host_id) else {
        return Some(Keep::Holds("its host can't be reached".to_string()));
    };
    let put_away = workspace.state == WorkspaceState::PutAway;
    if put_away {
        // Anything at its path, or git's record of a worktree there, is
        // what a put-away that failed part way left (R24).
        match workspace::git_worktree_gone(transport.as_ref(), &project.path, &workspace.path).await
        {
            Ok(true) => {}
            Ok(false) => {
                return Some(Keep::NoLeftover(
                    "a put-away that failed part way left something at its path",
                ));
            }
            Err(e) => {
                return Some(Keep::Holds(format!(
                    "whether anything is left at its path can't be told: {e}"
                )));
            }
        }
    } else {
        // A folder that is missing counts nothing, which is right for one
        // deleted by hand and wrong for one on a drive that is not plugged
        // in: there, with the folder above it gone too, it can't be checked.
        match workspace::within_reach(std::path::Path::new(&workspace.path)).await {
            Ok(true) => {}
            Ok(false) => {
                return Some(Keep::Holds("its folder can't be reached".to_string()));
            }
            Err(e) => {
                return Some(Keep::Holds(format!(
                    "whether its folder can be reached can't be told: {e}"
                )));
            }
        }
    }
    let changes = match handlers::changes_of(core, workspace).await {
        Ok((changes, _)) => changes,
        Err(e) => {
            return Some(Keep::Holds(format!(
                "what it holds can't be counted: {}",
                e.0.message
            )));
        }
    };
    // First: without git's record every file counts as uncommitted, which
    // is not why it stays.
    if changes.unrecorded {
        return Some(Keep::Holds("git has no record of it".to_string()));
    }
    let counted = [
        (changes.uncommitted, "uncommitted files"),
        (changes.ignored, "ignored files"),
        (changes.unshared, "commits no other branch holds"),
    ];
    for (count, what) in counted {
        if count > 0 {
            return Some(Keep::Holds(format!("{what}: {count}")));
        }
    }
    // A put-away workspace with no folder and no record has nowhere to
    // hold a repository of its own.
    if !put_away {
        match workspace::git_holds_repository(transport.as_ref(), &project.path, &workspace.path)
            .await
        {
            Ok(false) => {}
            Ok(true) => {
                return Some(Keep::Holds(
                    "its folder holds a repository of its own".to_string(),
                ));
            }
            Err(e) => {
                return Some(Keep::Holds(format!(
                    "whether its folder holds a repository of its own can't be told: {e}"
                )));
            }
        }
    }
    None
}

/// Judges `workspace`, which no session refers to: removes it when it is a
/// leftover that holds nothing, and leaves it otherwise. `said` holds the
/// leftovers whose keeping this sweep has already written to the
/// diagnostic log, so each is written once (FLOW-CLEAN-UP-R08).
pub(super) async fn judge(core: &Core, workspace: &Workspace, said: &mut HashSet<String>) {
    let id = &workspace.id;
    // Under way: a request is making or removing it.
    if matches!(
        workspace.state,
        WorkspaceState::Creating | WorkspaceState::Removing
    ) {
        return;
    }
    let made = match core.store.workspace_made_at(id.clone()).await {
        Ok(Some(made)) => made,
        Ok(None) => {
            kept(said, workspace, "when it was made can't be read");
            return;
        }
        Err(e) => {
            tracing::warn!(workspace = %id, error = %e, "the sweep could not read the workspace");
            return;
        }
    };
    // Younger, it may be a start still under way: no leftover yet.
    if core.sweep_now() - made < LEFTOVER_AGE {
        return;
    }
    match keeps(core, workspace).await {
        Some(Keep::NoLeftover(why)) => {
            tracing::debug!(workspace = %id, why, "the sweep left the workspace");
            return;
        }
        Some(Keep::Holds(why)) => {
            kept(said, workspace, &why);
            return;
        }
        None => {}
    }
    // Admission refuses it if a session got there since the rows were
    // read, and the removal judges it again in the project's turn. A
    // removal that fails leaves it for the next pass to judge again
    // (FLOW-CLEAN-UP-R25); no client is told.
    match handlers::remove_workspace(core, workspace, Remover::Sweep).await {
        Ok(()) => {
            said.remove(id);
            tracing::info!(
                workspace = %id,
                project = %workspace.project_id,
                path = %workspace.path,
                branch = %workspace.vcs_ref,
                "the sweep removed a leftover workspace: its folder, git's record and its branch"
            );
        }
        Err(e) => kept(said, workspace, &e.0.message),
    }
}

/// Writes why a leftover was kept to the diagnostic log the first time
/// this sweep keeps it (FLOW-CLEAN-UP-R08); after that only for whoever
/// reads the debug log.
fn kept(said: &mut HashSet<String>, workspace: &Workspace, why: &str) {
    if said.insert(workspace.id.clone()) {
        tracing::info!(
            workspace = %workspace.id,
            project = %workspace.project_id,
            path = %workspace.path,
            why,
            "the sweep kept a leftover workspace"
        );
    } else {
        tracing::debug!(workspace = %workspace.id, why, "the sweep kept a leftover workspace");
    }
}

/// Removes the project's folder under the worktrees root when it is empty,
/// as it is once its last workspace is gone. In the turn of every project
/// whose workspaces go in that folder (projects of one name share it), so
/// no `workspace.create`, fork or put-back is adding a worktree there
/// meanwhile.
pub(super) async fn tidy_project_folder(core: &Core, project: &Project) {
    let stored = match core.store.settings().await {
        Ok(stored) => stored,
        Err(e) => {
            tracing::warn!(error = %e, "the sweep could not read the settings");
            return;
        }
    };
    let root = settings::workspaces_root(&stored, &core.config.data_dir);
    let folder = root.join(&project.name);
    // Looked at first, so a project with workspaces takes no turn.
    if !matches!(workspace::is_empty_folder(&folder).await, Ok(true)) {
        return;
    }
    let Ok(projects) = core.store.projects().await else {
        return;
    };
    // Compared as the file system compares names: without case on Windows.
    let key = |name: &str| platform::path_key(&root.join(name).display().to_string());
    let mut sharing: Vec<String> = projects
        .into_iter()
        .filter(|other| key(&other.name) == key(&project.name))
        .map(|other| other.id)
        .collect();
    // Always taken in one order. No other request holds two turns at once.
    sharing.sort();
    let mut turns = Vec::new();
    for id in &sharing {
        let asked = RepositoryTurns { core, project: id };
        match asked.turn().await {
            Ok(turn) => turns.push(turn),
            Err(_) => return,
        }
    }
    match workspace::remove_empty_folder(&folder).await {
        Ok(true) => tracing::info!(
            project = %project.id,
            path = %folder.display(),
            "the sweep removed the project's empty folder under the worktrees root"
        ),
        Ok(false) => {}
        Err(e) => tracing::debug!(
            project = %project.id,
            path = %folder.display(),
            error = %e,
            "the sweep could not remove the project's empty folder"
        ),
    }
}
