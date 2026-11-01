//! Raw log lines: the persisted truth of a session (ADR 0010).

use serde::{Deserialize, Serialize};

/// One line of a session's raw log, exactly as it crossed the backend's stdio.
///
/// Core creates these, not adapters: an adapter hands Core the direction and
/// text through [`AdapterSink::record`](crate::AdapterSink::record), and Core
/// assigns `index` and `ts` as it appends to the log. One writer stamping every
/// line is what keeps `index` gap-free and in the order the lines happened when
/// the read loop and a host write race each other.
///
/// The serialized form is the `session.raw_log` entry in `PROTO-CORE-API`:
/// `{ index, ts, dir, line }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawLine {
    /// Position in the session's raw log: 0, 1, 2, … with no gaps, across both
    /// directions. Events cite these in `source.raw_from` and `source.raw_to`.
    pub index: u64,
    /// RFC 3339 UTC time at which the raw log recorded the line. An event's `ts`
    /// is copied from here, never taken from the clock at projection time, so
    /// projecting the same log twice gives the same events.
    pub ts: String,
    pub dir: Direction,
    /// The line verbatim, without its line terminator.
    pub line: String,
}

/// Which way a raw line went.
///
/// The raw log records both directions so that everything shown about a
/// session, including the user's messages and permission answers, can be
/// projected from the log alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The backend wrote it to its stdout.
    Backend,
    /// Ersa wrote it to the backend's stdin.
    Host,
}
