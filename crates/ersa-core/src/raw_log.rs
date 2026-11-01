//! The raw log: the lines exchanged with a live session's backend since Core
//! opened it, in both directions, held in memory for the developer pane and
//! never stored (ADR 0017). It is not a source of truth: a session's history
//! is the backend's own session record, and Core's lifecycle facts are in the
//! app log.
//!
//! Indices count every line Core received for the session in this process,
//! so a line keeps its index for as long as it is held. Memory is bounded:
//! past [`MAX_BYTES`] of line text, the oldest lines are let go, and a read
//! that starts before the first line still held starts there instead.

use std::collections::VecDeque;

use ersa_adapter::RawLine;

/// The most bytes of line text one session's raw log holds. A long session
/// with streaming replies writes megabytes; the developer pane shows recent
/// lines, and Core's memory must not grow with every session's whole life.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// One session's raw log.
#[derive(Debug)]
pub struct RawLog {
    lines: VecDeque<RawLine>,
    /// Bytes of line text held.
    bytes: usize,
    max_bytes: usize,
}

impl Default for RawLog {
    fn default() -> Self {
        RawLog::with_limit(MAX_BYTES)
    }
}

impl RawLog {
    /// A raw log that holds at most `max_bytes` of line text (and always the
    /// latest line, however long).
    pub fn with_limit(max_bytes: usize) -> Self {
        RawLog {
            lines: VecDeque::new(),
            bytes: 0,
            max_bytes,
        }
    }

    /// Appends `line`, letting the oldest lines go when over the limit.
    pub fn push(&mut self, line: RawLine) {
        self.bytes += line.line.len();
        self.lines.push_back(line);
        while self.bytes > self.max_bytes && self.lines.len() > 1 {
            if let Some(old) = self.lines.pop_front() {
                self.bytes -= old.line.len();
            }
        }
    }

    /// Lines `from..=to` still held, at most `limit` of them and about
    /// `max_bytes` of text (always at least one line when any is in range).
    pub fn read(&self, from: u64, to: u64, limit: usize, max_bytes: usize) -> Vec<RawLine> {
        let mut out = Vec::new();
        let mut bytes = 0;
        // Indices are consecutive, so the first wanted line's place is a
        // subtraction rather than a search.
        let first = self.lines.front().map_or(0, |line| line.index);
        let skip = usize::try_from(from.saturating_sub(first)).unwrap_or(usize::MAX);
        for line in self.lines.iter().skip(skip) {
            if line.index > to || out.len() >= limit {
                break;
            }
            if !out.is_empty() && bytes + line.line.len() > max_bytes {
                break;
            }
            bytes += line.line.len();
            out.push(line.clone());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ersa_adapter::Direction;

    fn line(index: u64, text: &str) -> RawLine {
        RawLine {
            index,
            ts: "2026-11-01T00:00:00.000Z".into(),
            dir: Direction::Backend,
            line: text.into(),
        }
    }

    fn indices(lines: &[RawLine]) -> Vec<u64> {
        lines.iter().map(|l| l.index).collect()
    }

    #[test]
    fn reads_ranges_with_limits() {
        let mut log = RawLog::default();
        for i in 0..10 {
            log.push(line(i, "abcd"));
        }
        assert_eq!(indices(&log.read(2, 4, 100, 1000)), [2, 3, 4]);
        assert_eq!(indices(&log.read(8, u64::MAX, 100, 1000)), [8, 9]);
        assert_eq!(indices(&log.read(0, u64::MAX, 3, 1000)), [0, 1, 2]);
        // Bytes cap the read, but one line always comes back.
        assert_eq!(indices(&log.read(0, u64::MAX, 100, 9)), [0, 1]);
        assert_eq!(indices(&log.read(0, u64::MAX, 100, 1)), [0]);
        assert!(log.read(10, u64::MAX, 100, 1000).is_empty());
    }

    #[test]
    fn the_oldest_lines_go_past_the_limit_and_indices_stay() {
        let mut log = RawLog::with_limit(10);
        for i in 0..5 {
            log.push(line(i, "abcd"));
        }
        // 20 bytes pushed, 10 held: the last two lines.
        assert_eq!(indices(&log.read(0, u64::MAX, 100, 1000)), [3, 4]);
        // A line longer than the limit is still held, alone.
        log.push(line(5, &"x".repeat(50)));
        assert_eq!(indices(&log.read(0, u64::MAX, 100, 1000)), [5]);
    }
}
