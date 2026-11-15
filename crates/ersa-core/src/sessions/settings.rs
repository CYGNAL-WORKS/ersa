//! A session's model, effort and permission mode as `session.list` reports
//! them: the backend's newest report of each (PROTO-CORE-API-R71).
//!
//! Three sources, newest first: what the live stream has reported (model and
//! permission mode only; the stream says nothing of effort), what the
//! record's turns ran with after Core last asked for a value, and Core's own
//! request. Core keeps only the request, with when it made it; the other two
//! are the backend's, read and held in memory.

use ersa_adapter::{Recorded, RecordedSettings, RelaunchSettings, Requested, SessionControl};
use ersa_protocol::core_api::Session;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{SEND_SETTLE, SessionError, SessionManager, bounded, lock};
use crate::persistence::{SessionRow, now_rfc3339};

impl SessionManager {
    /// Refuses a session that is not live as [`SessionManager::change`] would,
    /// so a stopped session is answered before its value is checked, which
    /// may wait on detecting the backend.
    pub fn check_live(&self, id: &str) -> Result<(), SessionError> {
        self.live(id).map(drop)
    }

    /// Changes a live session's model, effort or permission mode through its
    /// backend, at once (PROTO-CORE-API-R73, R74, R82), and keeps the new
    /// value with the time Core gave it, as any request is kept
    /// (PROTO-CORE-API-R71): a relaunch before a turn has run with it still
    /// passes it. A session that is not live is refused; one still starting
    /// takes the change once its backend is ready. Core has checked the
    /// value against `backend.list`.
    pub async fn change(&self, id: &str, control: SessionControl) -> Result<Session, SessionError> {
        let handle = self.handle(id)?;
        let live = self.live(id)?;
        let session = live.wait_ready().await?;
        // The time Core gives the backend the value, taken before it does:
        // a turn that began after it is one the backend may have run with
        // it, and none before it is.
        let at = now_rfc3339();
        let sent = control.clone();
        bounded(async move { session.change(sent).await }).await?;
        // The backend has it: keep it, so a relaunch passes it. Stored only
        // now, so a value the backend refused is never kept.
        let (name, value) = (control.name(), control.value().to_string());
        self.store
            .set_session_control(id.to_string(), name, value.clone(), at.clone())
            .await?;
        {
            let mut guard = lock(&handle.row);
            // A plain reference, so two of its fields can be borrowed at once.
            let row = &mut *guard;
            let (stored, stored_at) = match control {
                SessionControl::Model(_) => (&mut row.model, &mut row.model_at),
                SessionControl::Effort(_) => (&mut row.effort, &mut row.effort_at),
                SessionControl::PermissionMode(_) => {
                    (&mut row.permission_mode, &mut row.permission_mode_at)
                }
            };
            *stored = Some(value);
            *stored_at = Some(at);
        }
        // The adapter recorded the backend's answer before it returned, and
        // the stream's report of the new model or mode comes with it: wait
        // until the pipeline has projected it, so the session is listed
        // with the backend's own word on the new value. A pipeline that
        // falls behind only delays the report, which `session.list` gives
        // once projected.
        let target = live.sink.next_index();
        let mut progress = live.progress.clone();
        let _ = tokio::time::timeout(SEND_SETTLE, progress.wait_for(|p| p.lines >= target)).await;
        Ok(handle.describe())
    }
}

/// What a live session's stream last reported, kept after the run ends until
/// the record is read again, since the record then holds the same.
#[derive(Debug, Clone, Default)]
pub(super) struct Reported {
    pub model: Option<String>,
    pub permission_mode: Option<String>,
}

/// The three values as `session.list` reports them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Listed {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_mode: Option<String>,
}

/// The session's values: the stream's report, else the record's from a turn
/// after Core's request (or any turn for a value Core never requested), else
/// that request, absent when none was named.
pub(super) fn listed(row: &SessionRow, recorded: &RecordedSettings, reported: &Reported) -> Listed {
    let newer = recorded.clone().since(requested(row));
    let value = |recorded: Option<ersa_adapter::Recorded>| recorded.map(|r| r.value);
    Listed {
        model: reported
            .model
            .clone()
            .or_else(|| value(newer.model))
            .or_else(|| row.model.clone()),
        effort: value(newer.effort).or_else(|| row.effort.clone()),
        permission_mode: reported
            .permission_mode
            .clone()
            .or_else(|| value(newer.permission_mode))
            .or_else(|| row.permission_mode.clone()),
    }
}

/// The values a relaunch passes the backend (PROTO-CORE-API-R71). For a
/// value Core gave a time for: its request while no turn after it ran with
/// another value than the first such turn did, so a request for an alias or
/// the backend's default stays one even though the record names what it
/// resolved to; the record's latest once a later turn ran with another, as
/// when the user changed it in the backend's own tools. A value left to the
/// backend is a request for its default with a time like any other, so it
/// passes nothing until the record shows a change (PROTO-CORE-API-R98). A
/// value Core holds no time for has no first turn to weigh the latest
/// against: the record's latest from any turn, else what Core stored
/// (PROTO-CORE-API-R99).
pub(super) fn relaunched(row: &SessionRow, record: &RelaunchSettings) -> Listed {
    let requested = requested(row);
    let newer = record.latest.clone().since(requested);
    let pick = |request: &Option<String>,
                at: Option<OffsetDateTime>,
                newer: Option<Recorded>,
                first: Option<Recorded>| {
        let latest = newer.map(|r| r.value);
        match at {
            // Unchanged since the first turn after the request, or no turn
            // since: the request.
            Some(_) if latest.is_none() || latest == first.map(|r| r.value) => request.clone(),
            _ => latest.or_else(|| request.clone()),
        }
    };
    let first = record.first.clone();
    Listed {
        model: pick(&row.model, requested.model, newer.model, first.model),
        effort: pick(&row.effort, requested.effort, newer.effort, first.effort),
        permission_mode: pick(
            &row.permission_mode,
            requested.permission_mode,
            newer.permission_mode,
            first.permission_mode,
        ),
    }
}

/// When Core last gave the backend each value. A value stored without a
/// time, from a Core before it kept one, counts as never requested, so any
/// turn of the record is newer than it.
pub(super) fn requested(row: &SessionRow) -> Requested {
    let at = |text: &Option<String>| {
        text.as_deref()
            .and_then(|t| OffsetDateTime::parse(t, &Rfc3339).ok())
    };
    Requested {
        model: at(&row.model_at),
        effort: at(&row.effort_at),
        permission_mode: at(&row.permission_mode_at),
    }
}
