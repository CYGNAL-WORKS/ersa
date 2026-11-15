//! A session's model, effort and permission mode as `session.list` reports
//! them: the backend's newest report of each (PROTO-CORE-API-R71).
//!
//! Three sources, newest first: what the live stream has reported (model and
//! permission mode only; the stream says nothing of effort), what the
//! record's turns ran with after Core last asked for a value, and Core's own
//! request. Core keeps only the request, with when it made it; the other two
//! are the backend's, read and held in memory.

use ersa_adapter::{Recorded, RecordedSettings, RelaunchSettings, Requested};
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
/// that request, absent when none was named. A model the backend reports by
/// `resolves`, the id the backend says Core's model request resolves to, is
/// that request: the backend names an alias such as `opus` by the model it
/// chose for it, and the session controls show the choice the user made,
/// which a choice resolving to the same id would otherwise take.
pub(super) fn listed(
    row: &SessionRow,
    recorded: &RecordedSettings,
    reported: &Reported,
    resolves: Option<&str>,
) -> Listed {
    let requested = requested(row);
    let newer = recorded.clone().since(requested);
    let value = |recorded: Option<ersa_adapter::Recorded>| recorded.map(|r| r.value);
    let said = reported.model.clone().or_else(|| value(newer.model));
    let model = match said {
        Some(said) if requested.model.is_some() && resolves == Some(said.as_str()) => {
            row.model.clone()
        }
        said => said.or_else(|| row.model.clone()),
    };
    Listed {
        model,
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
