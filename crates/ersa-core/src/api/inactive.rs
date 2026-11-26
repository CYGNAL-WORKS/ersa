//! The sweep's archiving of inactive sessions (FLOW-CLEAN-UP Step 4,
//! FLOW-CLEAN-UP-R10): while `sessions.archive_inactive_days` is set, each
//! pass archives the stopped sessions with no activity for that many days,
//! before it judges the workspaces, so a folder an archive frees is put away
//! by the same pass (R22). It stops no session: one still running is
//! archived by a later pass, once it has stopped. Unset, which is the
//! default, it archives none and reads nothing for it.

use super::sweep;
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
            // Started, pinned or opened since it was listed, or refused as
            // `session.set_archived` refuses one: left as it is.
            Err(e) => {
                tracing::debug!(session = %id, error = %e, "the sweep left the session unarchived")
            }
        }
    }
}

/// Why the session must stay because another process may have it open,
/// if it must, as the put-away asks (FLOW-CLEAN-UP-R22). Every candidate
/// was stopped when listed; one Core has started since shows its own
/// process in the registry and stays, as the archive's second look would
/// leave it anyway.
async fn open_elsewhere(core: &Core, id: &str) -> Option<&'static str> {
    let Ok(session) = core.sessions.get(id) else {
        return Some("the session is gone");
    };
    sweep::open_elsewhere(core, id, &session.host_id).await
}
