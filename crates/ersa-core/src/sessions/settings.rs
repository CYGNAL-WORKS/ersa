//! A session's model, effort and permission mode as `session.list` reports
//! them: the backend's newest report of each (PROTO-CORE-API-R71).
//!
//! Three sources, newest first: what the live stream has reported (model and
//! permission mode only; the stream says nothing of effort), what the
//! record's turns ran with after Core last asked for a value, and Core's own
//! request. Core keeps only the request, with when it made it; the other two
//! are the backend's, read and held in memory.

use ersa_adapter::{RecordedSettings, Requested};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::persistence::SessionRow;

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

/// When Core last gave the backend each value. A value stored without a
/// time, from a Core before it kept one, counts as never requested, so any
/// turn of the record is newer than it.
fn requested(row: &SessionRow) -> Requested {
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
