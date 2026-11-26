//! Which sessions the sweep archives while Archive inactive sessions is on
//! (FLOW-CLEAN-UP Step 4, FLOW-CLEAN-UP-R10): each one listed that is not
//! archived, pinned, unseen or marked deleting, that no connection is
//! subscribed to, and whose last activity, else its creation, is at least
//! the chosen number of days ago. Claude Desktop's own words for it are "no
//! activity and no attention": a subscription is a window showing the
//! session, and pinned and unseen are the user's attention.
//!
//! The archive itself is `session.set_archived`'s, so a busy session is
//! refused and left, and a live one is stopped first.

use ersa_protocol::core_api::Session;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{Handle, SessionError, SessionManager, UNKNOWN_TIME, lock};

impl SessionManager {
    /// The sessions the sweep would archive now, by id, for an inactivity
    /// that began at `cutoff` or earlier.
    pub fn inactive_since(&self, cutoff: OffsetDateTime) -> Vec<String> {
        self.handles()
            .iter()
            .filter(|handle| keeps(handle, cutoff).is_none())
            .map(|handle| handle.id.clone())
            .collect()
    }

    /// Archives session `id` as `session.set_archived` does, unless by the
    /// time the archive holds the session's turn it is no longer one the
    /// sweep archives: pinned, opened or used since it was listed, say.
    /// Refused as `session.set_archived` refuses it otherwise.
    pub async fn archive_inactive(
        &self,
        id: &str,
        cutoff: OffsetDateTime,
    ) -> Result<Session, SessionError> {
        self.set_archived_unless(id, true, |handle| keeps(handle, cutoff).map(String::from))
            .await
    }
}

/// Why the sweep keeps session `handle` unarchived, if it does.
fn keeps(handle: &Handle, cutoff: OffsetDateTime) -> Option<&'static str> {
    let Some(session) = handle.describe() else {
        return Some("it is gone");
    };
    // A window showing the session holds a subscription to it.
    let watched = !lock(&handle.hub).subscribers.is_empty();
    why_kept(&session, watched, cutoff)
}

/// Why `session` is kept, if it is, `watched` saying whether a connection
/// is subscribed to it (FLOW-CLEAN-UP-R10).
fn why_kept(session: &Session, watched: bool, cutoff: OffsetDateTime) -> Option<&'static str> {
    if session.archived {
        return Some("it is archived");
    }
    if session.pinned {
        return Some("it is pinned");
    }
    if session.unseen {
        return Some("it is unseen");
    }
    if session.deleting.is_some() {
        return Some("it is marked deleting");
    }
    if watched {
        return Some("a connection is subscribed to it");
    }
    match last_active(session) {
        Some(at) if at <= cutoff => None,
        Some(_) => Some("it was active since"),
        // Nothing says when it was last active or made: kept, as the sweep
        // keeps anything it could not check (FLOW-CLEAN-UP-R08).
        None => Some("its last activity and its creation are unknown"),
    }
}

/// When `session` was last active: its last activity, else its creation.
/// A creation nothing remembers is no time at all.
fn last_active(session: &Session) -> Option<OffsetDateTime> {
    let parse = |at: &str| OffsetDateTime::parse(at, &Rfc3339).ok();
    match &session.last_activity_at {
        Some(at) => parse(at),
        None if session.created_at == UNKNOWN_TIME => None,
        None => parse(&session.created_at),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ersa_protocol::core_api::Deleting;

    /// A session as `session.list` lists one that is not archived, pinned,
    /// unseen or marked deleting, made at `created_at` and last active at
    /// `last_activity_at`.
    fn session(created_at: &str, last_activity_at: Option<&str>) -> Session {
        serde_json::from_value(serde_json::json!({
            "id": "s-1",
            "workspace_id": "w-1",
            "host_id": "local",
            "backend": "fake",
            "permission_mode": "ask",
            "mode": "",
            "state": "ended",
            "created_at": created_at,
            "last_activity_at": last_activity_at,
            "pinned": false,
            "archived": false,
            "unseen": false,
            "tile": { "state": "ended", "pending": [] },
        }))
        .expect("a session as session.list lists one")
    }

    /// The time the tests count back to.
    fn cutoff() -> OffsetDateTime {
        OffsetDateTime::parse("2026-11-19T12:00:00.000Z", &Rfc3339).unwrap()
    }

    /// Archived once its last activity is at the cutoff or before, never
    /// after; its creation counts only when it has no last activity.
    // verifies: FLOW-CLEAN-UP-R10
    #[test]
    fn a_session_is_archived_from_the_cutoff_on_by_its_last_activity_else_its_creation() {
        let old = "2026-11-01T00:00:00.000Z";
        let at_cutoff = "2026-11-19T12:00:00.000Z";
        let after = "2026-11-19T12:00:00.001Z";
        assert_eq!(
            why_kept(&session(old, Some(at_cutoff)), false, cutoff()),
            None
        );
        assert_eq!(
            why_kept(&session(old, Some(after)), false, cutoff()),
            Some("it was active since")
        );
        // No last activity: its creation.
        assert_eq!(why_kept(&session(at_cutoff, None), false, cutoff()), None);
        assert_eq!(
            why_kept(&session(after, None), false, cutoff()),
            Some("it was active since")
        );
        // A creation nothing remembers is not a time.
        assert_eq!(
            why_kept(&session(UNKNOWN_TIME, None), false, cutoff()),
            Some("its last activity and its creation are unknown")
        );
    }

    /// Archived, pinned, unseen, marked deleting and subscribed to each keep
    /// a session however long ago it was active.
    // verifies: FLOW-CLEAN-UP-R10
    #[test]
    fn attention_keeps_a_session_however_old() {
        let old = session("2026-01-01T00:00:00.000Z", None);
        assert_eq!(why_kept(&old, false, cutoff()), None);
        let kept = |change: fn(&mut Session)| {
            let mut session = old.clone();
            change(&mut session);
            why_kept(&session, false, cutoff())
        };
        assert_eq!(kept(|s| s.archived = true), Some("it is archived"));
        assert_eq!(kept(|s| s.pinned = true), Some("it is pinned"));
        assert_eq!(kept(|s| s.unseen = true), Some("it is unseen"));
        assert_eq!(
            kept(|s| s.deleting = Some(Deleting::Failed)),
            Some("it is marked deleting")
        );
        assert_eq!(
            why_kept(&old, true, cutoff()),
            Some("a connection is subscribed to it")
        );
    }
}
