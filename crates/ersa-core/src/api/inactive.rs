//! The sweep's archiving of inactive sessions (FLOW-CLEAN-UP Step 4,
//! FLOW-CLEAN-UP-R10): while `sessions.archive_inactive_days` is set, each
//! pass archives the sessions with no activity for that many days, as
//! `session.set_archived` does, before it judges the workspaces, so a folder
//! an archive frees is put away by the same pass (R22). Unset, which is the
//! default, it archives none and reads nothing for it.

use ersa_adapter::OpenElsewhere;

use crate::settings;
use crate::state::Core;

/// Archives each session inactive for the days the setting names.
pub(super) async fn archive(core: &Core) {
    let stored = match core.store.settings().await {
        Ok(stored) => stored,
        Err(e) => {
            tracing::warn!(error = %e, "the sweep could not read the settings; it archived nothing");
            return;
        }
    };
    let Some(days) = settings::archive_inactive_days(&stored) else {
        return;
    };
    // Turns taken outside Ersa count as activity, as `session.list` shows
    // them (PROTO-CORE-API-R75).
    core.sessions
        .reread_records(|host| core.session_transport_for(host))
        .await;
    // A day is 24 hours on the sweep's clock: the setting counts days of
    // no activity, not calendar dates. `days` is at most 30.
    let cutoff = core.sweep_now() - time::Duration::days(days as i64);
    for id in core.sessions.inactive_since(cutoff) {
        if core.shutting_down() {
            return;
        }
        if let Some(why) = open_elsewhere(core, &id).await {
            tracing::debug!(session = %id, why, "the sweep left the session unarchived");
            continue;
        }
        match core.sessions.archive_inactive(&id, cutoff).await {
            // For whoever wonders later why a session was archived.
            Ok(session) => tracing::info!(
                session = %id,
                workspace = %session.workspace_id,
                days,
                "the sweep archived the session: it had no activity for that many days"
            ),
            // Busy, pinned or opened since it was listed, or refused as
            // `session.set_archived` refuses one: left as it is.
            Err(e) => {
                tracing::debug!(session = %id, error = %e, "the sweep left the session unarchived")
            }
        }
    }
}

/// Why the session must stay because another process may have it open,
/// if it must. A session Core runs is open in Core's own process, which the
/// backend's registry cannot tell from another, so only a stopped one is
/// asked about.
async fn open_elsewhere(core: &Core, id: &str) -> Option<&'static str> {
    match core.sessions.is_running(id).await {
        Ok(true) => return None,
        Ok(false) => {}
        Err(_) => return Some("it is gone"),
    }
    let host = core.sessions.get(id).ok()?.host_id;
    let Some(transport) = core.session_transport_for(&host) else {
        return Some("it is on a host Core can't reach");
    };
    match core.sessions.open_elsewhere(id, transport).await {
        Ok(OpenElsewhere::No) => None,
        Ok(OpenElsewhere::Yes { .. }) => Some("another process has it open"),
        Ok(OpenElsewhere::Unknown) | Err(_) => {
            Some("whether another process has it open can't be told")
        }
    }
}
