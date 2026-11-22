//! The raw log: the lines exchanged with a live session's backend since Core
//! opened it, in both directions, held in memory for the developer pane and
//! never stored (ADR 0017). It is not a source of truth: a session's history
//! is the backend's own session record, and Core's lifecycle facts are in the
//! app log.
//!
//! Indices count every line Core exchanged for the session in this process,
//! across every run of its backend and every time Core let the log go, so an
//! index is never used twice (PROTO-CORE-API-R136); the session's handle
//! keeps the count, and this log only holds the lines. Memory is bounded:
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

    /// Whether the log holds no line.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Entries for the lines `from..=to` still held, at most `limit` of them
    /// and at most `max_bytes` by their cost. `entry` turns a line into what
    /// the caller sends and says what it costs, so the bound is on what is
    /// sent rather than on the line's text (PROTO-CORE-API-R139). The first
    /// entry always comes back, however much it costs.
    pub fn read<T>(
        &self,
        from: u64,
        to: u64,
        limit: usize,
        max_bytes: usize,
        mut entry: impl FnMut(&RawLine) -> (T, usize),
    ) -> Vec<T> {
        let mut out = Vec::new();
        let mut bytes = 0;
        // Lines are held in index order, so the first wanted one is found by
        // a binary search rather than a walk from the front.
        let start = self.lines.partition_point(|line| line.index < from);
        for line in self.lines.range(start..) {
            if line.index > to || out.len() >= limit {
                break;
            }
            let (made, cost) = entry(line);
            if !out.is_empty() && bytes + cost > max_bytes {
                break;
            }
            bytes += cost;
            out.push(made);
        }
        out
    }
}

/// The longest start of `line` that is at most `clip` bytes and ends on a
/// whole character (PROTO-CORE-API-R138): a cut inside a character's bytes
/// would not be text.
pub fn head(line: &str, clip: usize) -> &str {
    &line[..line.floor_char_boundary(clip)]
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

    /// The indices a read returns, each line costing its text's length.
    fn read(log: &RawLog, from: u64, to: u64, limit: usize, max_bytes: usize) -> Vec<u64> {
        log.read(from, to, limit, max_bytes, |l| (l.index, l.line.len()))
    }

    #[test]
    fn reads_ranges_with_limits() {
        let mut log = RawLog::default();
        for i in 0..10 {
            log.push(line(i, "abcd"));
        }
        assert_eq!(read(&log, 2, 4, 100, 1000), [2, 3, 4]);
        assert_eq!(read(&log, 8, u64::MAX, 100, 1000), [8, 9]);
        assert_eq!(read(&log, 0, u64::MAX, 3, 1000), [0, 1, 2]);
        // Bytes cap the read, but one line always comes back.
        assert_eq!(read(&log, 0, u64::MAX, 100, 9), [0, 1]);
        assert_eq!(read(&log, 0, u64::MAX, 100, 1), [0]);
        assert!(read(&log, 10, u64::MAX, 100, 1000).is_empty());
    }

    #[test]
    fn the_oldest_lines_go_past_the_limit_and_indices_stay() {
        let mut log = RawLog::with_limit(10);
        for i in 0..5 {
            log.push(line(i, "abcd"));
        }
        // 20 bytes pushed, 10 held: the last two lines.
        assert_eq!(read(&log, 0, u64::MAX, 100, 1000), [3, 4]);
        // A line longer than the limit is still held, alone.
        log.push(line(5, &"x".repeat(50)));
        assert_eq!(read(&log, 0, u64::MAX, 100, 1000), [5]);
    }

    #[test]
    fn a_read_is_bounded_by_what_its_entries_cost() {
        let mut log = RawLog::default();
        for i in 0..4 {
            log.push(line(i, "ab"));
        }
        // Each entry costs ten times its text: the bound is on the cost the
        // caller states, not on the line's length.
        let costed = |cap| log.read(0, u64::MAX, 100, cap, |l| (l.index, 10 * l.line.len()));
        assert_eq!(costed(40), [0, 1]);
        assert_eq!(costed(39), [0]);
        // One entry comes back even when it alone costs more.
        assert_eq!(costed(5), [0]);
    }

    // verifies: PROTO-CORE-API-R138
    #[test]
    fn a_head_ends_on_a_whole_character() {
        // "é" is two bytes: a cut at 3 would split the second one.
        assert_eq!(head("éé", 3), "é");
        assert_eq!(head("éé", 4), "éé");
        assert_eq!(head("abcdef", 4), "abcd");
        // "€" is three bytes: cuts at 1 and 2 keep nothing of it.
        assert_eq!(head("€x", 2), "");
        assert_eq!(head("a€", 3), "a");
    }
}
