//! The sweep (FLOW-CLEAN-UP, The sweep): Core's own pass, when it starts,
//! every hour after that and after a session is archived, that archives the
//! sessions inactive for as long as the user chose, if they chose
//! ([`inactive`], R10), puts away each isolated workspace whose sessions are
//! all archived and that holds no unsaved work (FLOW-CLEAN-UP-R22), and
//! removes each leftover workspace that holds nothing ([`leftover`], R23).
//! It asks nothing and tells no client, beyond the tile an archive sends.
//!
//! Every way it can't be sure is a reason to leave the folder: a count that
//! fails, a session it can't load, a process it can't rule out. The
//! put-away itself refuses again what changed since (PROTO-CORE-API-R140,
//! R147), leaving the workspace `ready` for the next pass (R24).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::OpenElsewhere;
use ersa_protocol::core_api::{Workspace, WorkspaceKind, WorkspaceState};
use tokio::sync::mpsc;

use super::{handlers, inactive, leftover, put_away};
use crate::persistence::SessionRow;
use crate::state::{Core, SWEEP_INTERVAL, Sweeps};
use crate::workspace;

/// How long a pass waits for a session's run that is still winding down, as
/// one just archived may be, before it leaves the workspace for later.
const RUN_SETTLING: Duration = Duration::from_secs(30);

/// Sweeps when Core starts, then each time an hour passes or a sweep is
/// asked for (FLOW-CLEAN-UP-R06), one pass at a time, until Core shuts
/// down. Asked for during a pass, the next pass follows it.
pub(crate) async fn run(core: Arc<Core>) {
    let clock = match &core.config.sweeps {
        Sweeps::Never => return,
        Sweeps::Hourly => None,
        Sweeps::Driven(clock) => Some(Arc::clone(clock)),
    };
    // The first tick an hour from now: the pass at start runs below.
    let start = tokio::time::Instant::now() + SWEEP_INTERVAL;
    let mut hourly = tokio::time::interval_at(start, SWEEP_INTERVAL);
    // A pass that outlasts the hour is followed by one more, not by a burst.
    hourly.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // The leftovers whose keeping has been written to the diagnostic log,
    // once each while Core runs (FLOW-CLEAN-UP-R08).
    let mut said = HashSet::new();
    loop {
        sweep(&core, &mut said).await;
        if let Some(clock) = &clock {
            clock.sweep_ended();
        }
        let hour = async {
            match &clock {
                Some(clock) => clock.hour().await,
                None => {
                    hourly.tick().await;
                }
            }
        };
        tokio::select! {
            () = hour => {}
            () = core.sweep_asked() => {}
            () = core.shutdown_requested() => return,
        }
    }
}

/// One pass: each isolated workspace, one at a time, then the project's
/// folder under the worktrees root when that is left empty.
async fn sweep(core: &Core, said: &mut HashSet<String>) {
    // Sessions started in the backend's own tools use a folder as any
    // other does (FLOW-CLEAN-UP, Variations): found first, as `session.list`
    // finds them, so the pass at start, before any list, counts them too.
    if let Err(e) = core
        .sessions
        .find_elsewhere(|host| core.session_transport_for(host))
        .await
    {
        tracing::warn!(error = %e, "the sweep could not list the sessions; it put nothing away");
        return;
    }
    // First, so a folder an archive frees is put away by this same pass.
    inactive::archive(core).await;
    let projects = match core.store.projects().await {
        Ok(projects) => projects,
        Err(e) => {
            tracing::warn!(error = %e, "the sweep could not read the projects");
            return;
        }
    };
    for project in projects {
        let workspaces = match core.store.workspaces(project.id.clone()).await {
            Ok(workspaces) => workspaces,
            Err(e) => {
                tracing::warn!(project = %project.id, error = %e, "the sweep could not read the workspaces");
                continue;
            }
        };
        for workspace in workspaces {
            if core.shutting_down() {
                return;
            }
            if workspace.kind != WorkspaceKind::Isolated {
                continue;
            }
            alone(core, judge(core, &workspace.id, said)).await;
        }
        alone(core, leftover::tidy_project_folder(core, &project)).await;
    }
}

/// Runs `work`, then waits until every file system job it began has let
/// its place go, a job given up on at its time limit included:
/// so the sweep holds at most one of the places PROTO-CORE-API-R116 counts
/// (FLOW-CLEAN-UP-R06), and a folder that stopped answering stops the sweep
/// rather than take every place from the requests that need one. Shutdown
/// ends the wait.
async fn alone(core: &Core, work: impl Future<Output = ()>) {
    let (held, mut all_let_go) = mpsc::channel::<()>(1);
    workspace::carrying(Arc::new(held), work).await;
    tokio::select! {
        // `None` once the last job's thread has dropped what it carried.
        _ = all_let_go.recv() => {}
        () = core.shutdown_requested() => {}
    }
}

/// Puts workspace `id` away when FLOW-CLEAN-UP-R22 lets it, removes it when
/// it is a leftover that holds nothing (R23), and leaves it otherwise. Read
/// again here, not taken from the start of the pass, which may have been a
/// while ago.
async fn judge(core: &Core, id: &str, said: &mut HashSet<String>) {
    let workspace = match core.store.workspace(id.to_string()).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(workspace = %id, error = %e, "the sweep could not read the workspace");
            return;
        }
    };
    // A root workspace is the user's own folder.
    if workspace.kind != WorkspaceKind::Isolated {
        return;
    }
    let rows = match core.store.sessions().await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(workspace = %id, error = %e, "the sweep could not read the sessions");
            return;
        }
    };
    // Every row counts, whatever its session is doing: archived, dormant,
    // found in the backend's own tools or marked deleting.
    let there: Vec<SessionRow> = rows.into_iter().filter(|r| r.workspace_id == *id).collect();
    if there.is_empty() {
        leftover::judge(core, &workspace, said).await;
        return;
    }
    // One in any other state has no folder to put away, or one under way:
    // one left `put_away` by a put-away that failed part way is never tried
    // again (FLOW-CLEAN-UP-R24).
    if workspace.state != WorkspaceState::Ready {
        return;
    }
    if let Some(why) = sessions_keep(core, &there).await {
        tracing::debug!(workspace = %id, why, "the sweep left the workspace");
        return;
    }
    if let Some(why) = unsaved(core, &workspace).await {
        tracing::debug!(workspace = %id, %why, "the sweep left the workspace");
        return;
    }
    // Admission decides again that every session there is archived, as it
    // admits the put-away: one unarchived since the look above keeps it.
    match put_away::put_away_for_sweep(core, id).await {
        // For whoever wonders later where a folder went (FLOW-CLEAN-UP-R08).
        Ok(()) => tracing::info!(
            workspace = %id,
            project = %workspace.project_id,
            path = %workspace.path,
            branch = %workspace.vcs_ref,
            "the sweep put the workspace away: its folder was removed and its branch kept"
        ),
        // A refusal left it `ready` for the next pass, a failure part way
        // left it `put_away`, which no pass looks at again (FLOW-CLEAN-UP-R24);
        // the put-away wrote either to the diagnostic log. No client is told.
        Err(e) => {
            tracing::debug!(workspace = %id, error = %e.0.message, "the sweep did not put the workspace away")
        }
    }
}

/// Why the sessions in a workspace keep it, if they do (FLOW-CLEAN-UP-R22):
/// none uses it; one is not archived or is dormant; one's run is still
/// winding down; or another process has one open, or may have.
async fn sessions_keep(core: &Core, there: &[SessionRow]) -> Option<&'static str> {
    // One marked deleting, live or starting, admission refuses the
    // put-away over (its `has_session_in_use`).
    if there.iter().any(|row| !row.archived) {
        return Some("a session there is not archived");
    }
    // Admission never loads a session whose backend has no adapter here.
    if there
        .iter()
        .any(|row| core.sessions.adapter(&row.backend).is_none())
    {
        return Some("a session there is dormant");
    }
    for row in there {
        // An archive stops the session, and its run may still be ending.
        if core
            .admission
            .run_settled(&row.id, RUN_SETTLING)
            .await
            .is_err()
        {
            return Some("a session there is running");
        }
        if let Some(why) = open_elsewhere(core, &row.id, &row.host_id).await {
            return Some(why);
        }
    }
    None
}

/// Why session `id`, on host `host`, must be left because another process
/// may have it open, if it must: the backend's registry shows one, can't
/// say, or the host can't be reached. Asked only of a session Core does not
/// run, since the registry can't tell Core's own process from another.
pub(super) async fn open_elsewhere(core: &Core, id: &str, host: &str) -> Option<&'static str> {
    let Some(transport) = core.session_transport_for(host) else {
        return Some("the session is on a host Core can't reach");
    };
    match core.sessions.open_elsewhere(id, transport).await {
        Ok(OpenElsewhere::No) => None,
        Ok(OpenElsewhere::Yes { .. }) => Some("another process has the session open"),
        Ok(OpenElsewhere::Unknown) | Err(_) => {
            Some("whether another process has the session open can't be told")
        }
    }
}

/// Why the workspace's folder must stay, if it must: `workspace.changes`
/// counts uncommitted files or stranded commits, git has no record of it,
/// or the count failed (FLOW-CLEAN-UP-R22, R08). Ignored files go with the
/// folder; commits its branch holds stay with the branch.
async fn unsaved(core: &Core, workspace: &Workspace) -> Option<String> {
    match handlers::changes_of(core, workspace).await {
        Err(e) => {
            tracing::warn!(workspace = %workspace.id, error = %e.0.message, "the sweep could not count what the workspace holds, so it stays");
            Some("its count failed".to_string())
        }
        Ok((changes, _)) if changes.uncommitted > 0 => {
            Some(format!("{} uncommitted files", changes.uncommitted))
        }
        Ok((changes, _)) if changes.stranded > 0 => {
            Some(format!("{} commits no branch holds", changes.stranded))
        }
        Ok((changes, _)) if changes.unrecorded => Some("git has no record of it".to_string()),
        Ok(_) => None,
    }
}
