//! Recorded backend sessions: the fixtures protocol briefs are written from.
//!
//! A recording is two files side by side. `<name>.jsonl` holds the lines the
//! backend wrote to stdout, one per line. `<name>.meta.json` holds what the
//! recorder noted about the run: when each stdout line arrived
//! (`stdout_timing_ms`), the lines it wrote to the backend's stdin and when
//! (`sent`, each `{ t, line }`), and when it was recorded (`recorded_at`).
//! One-shot recordings have neither timings nor stdin lines.
//!
//! Adapters' golden tests and Core's replay server both turn a recording into
//! the raw log Core would have kept for that session, with
//! [`Recording::lines`].

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use crate::raw::{Direction, RawLine};

/// The longest a recording may run, in milliseconds: a day. A later time
/// is a broken file, and replaying it would wait for ever.
pub const MAX_TIME_MS: u64 = 24 * 60 * 60 * 1000;

/// One line that crossed the backend's stdio in a recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedLine {
    /// Milliseconds after the start at which it crossed.
    pub at_ms: u64,
    pub dir: Direction,
    /// The line verbatim, without its line terminator.
    pub line: String,
}

/// One recorded session, read whole and checked: a recording that cannot be
/// read faithfully is refused with the reason, never filled in with a
/// default.
#[derive(Debug, Clone)]
pub struct Recording {
    /// Every line, in the order Core would have recorded them.
    lines: Vec<TimedLine>,
    /// The recorder's other notes, verbatim: backend-specific keys (such as
    /// the backend's version) are read from here by whoever knows them. The
    /// lines and their times are in `lines`, not here.
    pub meta: Value,
}

impl Recording {
    /// Reads the recording whose stdout is `jsonl`; its notes are the
    /// `.meta.json` file beside it.
    pub fn load(jsonl: &Path) -> io::Result<Recording> {
        let stem = jsonl
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".jsonl"))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} is not a .jsonl file", jsonl.display()),
                )
            })?;
        let meta_path = jsonl.with_file_name(format!("{stem}.meta.json"));
        let meta = fs::read_to_string(&meta_path)?;
        let meta = serde_json::from_str(&meta).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", meta_path.display()),
            )
        })?;
        let stdout = fs::read_to_string(jsonl)?;
        Recording::parse(&stdout, meta)
    }

    /// A recording from the stdout file's text and the parsed notes.
    pub fn parse(stdout: &str, mut meta: Value) -> io::Result<Recording> {
        let notes = meta
            .as_object_mut()
            .ok_or_else(|| invalid("the notes are not a JSON object"))?;
        // Taken out of the notes, so the lines are held once.
        let timings = notes.remove("stdout_timing_ms");
        let sent = notes.remove("sent");

        let stdout: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
        let stdout_times: Vec<u64> = match timings {
            Some(Value::Array(times)) => times
                .iter()
                .map(|t| {
                    t.as_u64()
                        .ok_or_else(|| invalid("a stdout timing is not a whole number"))
                        .and_then(checked_time)
                })
                .collect::<io::Result<_>>()?,
            Some(_) => return Err(invalid("`stdout_timing_ms` is not an array")),
            None => Vec::new(),
        };
        let sent: Vec<(u64, String)> = match sent {
            Some(Value::Array(sent)) => sent
                .into_iter()
                .map(|s| match (s["t"].as_u64(), s["line"].as_str()) {
                    (Some(t), Some(line)) => Ok((checked_time(t)?, line.to_string())),
                    _ => Err(invalid(
                        "every `sent` entry needs a whole-number `t` and a `line`",
                    )),
                })
                .collect::<io::Result<_>>()?,
            Some(_) => return Err(invalid("`sent` is not an array")),
            None => Vec::new(),
        };

        // Without timings the stdout lines all count as at 0, which is
        // faithful only when nothing was written to stdin between them.
        let stdout_times = if stdout_times.is_empty() && !stdout.is_empty() {
            if !sent.is_empty() {
                return Err(invalid(
                    "stdin lines without stdout timings cannot be put in order",
                ));
            }
            vec![0; stdout.len()]
        } else {
            stdout_times
        };
        if stdout_times.len() != stdout.len() {
            return Err(invalid(&format!(
                "{} stdout timings for {} stdout lines",
                stdout_times.len(),
                stdout.len()
            )));
        }
        if !is_sorted(stdout_times.iter().copied()) || !is_sorted(sent.iter().map(|(t, _)| *t)) {
            return Err(invalid("the recorded times go backwards"));
        }

        // Stdout and stdin lines interleaved by time. A stdout line and a
        // stdin line at the same millisecond go stdout first, since the host
        // writes in reaction to what it read.
        let mut lines = Vec::with_capacity(stdout.len() + sent.len());
        let mut sent = sent.into_iter().peekable();
        let mut out = 0;
        while out < stdout.len() || sent.peek().is_some() {
            let take_stdout = match sent.peek() {
                Some((t, _)) => out < stdout.len() && stdout_times[out] <= *t,
                None => true,
            };
            if take_stdout {
                lines.push(TimedLine {
                    at_ms: stdout_times[out],
                    dir: Direction::Backend,
                    line: stdout[out].to_string(),
                });
                out += 1;
            } else if let Some((at_ms, line)) = sent.next() {
                lines.push(TimedLine {
                    at_ms,
                    dir: Direction::Host,
                    line,
                });
            }
        }
        Ok(Recording { lines, meta })
    }

    /// Every line that crossed the backend's stdio, in the order Core would
    /// have recorded them.
    pub fn lines(&self) -> &[TimedLine] {
        &self.lines
    }

    /// The lines, taken out of the recording.
    pub fn into_lines(self) -> Vec<TimedLine> {
        self.lines
    }

    /// When the recording was made, RFC 3339, if it says.
    pub fn recorded_at(&self) -> Option<&str> {
        self.meta["recorded_at"].as_str()
    }

    /// The lines the backend wrote to stdout, in order.
    pub fn stdout(&self) -> Vec<String> {
        self.lines_from(Direction::Backend)
    }

    /// The lines written to the backend's stdin, in order.
    pub fn sent(&self) -> Vec<String> {
        self.lines_from(Direction::Host)
    }

    fn lines_from(&self, dir: Direction) -> Vec<String> {
        self.lines
            .iter()
            .filter(|l| l.dir == dir)
            .map(|l| l.line.clone())
            .collect()
    }

    /// The session's raw log as Core would have recorded it, every line
    /// stamped with the recording's time.
    pub fn raw_log(&self) -> Vec<RawLine> {
        let ts = self.recorded_at().unwrap_or("2026-10-31T00:00:00Z");
        self.lines
            .iter()
            .enumerate()
            .map(|(index, l)| RawLine {
                index: index as u64,
                ts: ts.to_string(),
                dir: l.dir,
                line: l.line.clone(),
            })
            .collect()
    }
}

fn is_sorted(mut times: impl Iterator<Item = u64>) -> bool {
    let Some(mut last) = times.next() else {
        return true;
    };
    times.all(|t| {
        let in_order = t >= last;
        last = t;
        in_order
    })
}

fn checked_time(t: u64) -> io::Result<u64> {
    if t > MAX_TIME_MS {
        return Err(invalid(&format!("a time of {t} ms is past a day")));
    }
    Ok(t)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(stdout: &[&str], meta: Value) -> io::Result<Recording> {
        Recording::parse(&stdout.join("\n"), meta)
    }

    fn refused(stdout: &[&str], meta: Value) {
        let error = parse(stdout, meta.clone()).expect_err(&format!("{meta} is refused"));
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn lines_interleave_by_time_with_stdout_first_on_a_tie() {
        let r = parse(
            &["a", "b", "c"],
            json!({
                "stdout_timing_ms": [10, 20, 30],
                "sent": [{ "t": 5, "line": "x" }, { "t": 20, "line": "y" }]
            }),
        )
        .unwrap();
        let order: Vec<(u64, &str)> = r
            .lines()
            .iter()
            .map(|l| (l.at_ms, l.line.as_str()))
            .collect();
        assert_eq!(
            order,
            [(5, "x"), (10, "a"), (20, "b"), (20, "y"), (30, "c")]
        );
        assert_eq!(r.lines()[0].dir, Direction::Host);
        assert_eq!(r.lines()[1].dir, Direction::Backend);
        assert_eq!(r.stdout(), ["a", "b", "c"]);
        assert_eq!(r.sent(), ["x", "y"]);
        // The lines are held once, not also in the notes.
        assert_eq!(r.meta, json!({}));
    }

    #[test]
    fn a_one_shot_recording_is_its_stdout_at_zero() {
        let r = parse(&["a", "b"], json!({})).unwrap();
        let lines: Vec<(u64, &str)> = r
            .lines()
            .iter()
            .map(|l| (l.at_ms, l.line.as_str()))
            .collect();
        assert_eq!(lines, [(0, "a"), (0, "b")]);
    }

    #[test]
    fn a_recording_that_cannot_be_read_faithfully_is_refused() {
        refused(&["a", "b"], json!({ "stdout_timing_ms": [1] }));
        refused(&["a"], json!({ "stdout_timing_ms": "fast" }));
        refused(&["a"], json!({ "stdout_timing_ms": [1.5] }));
        refused(&["a"], json!({ "stdout_timing_ms": [1], "sent": "x" }));
        refused(
            &["a"],
            json!({ "stdout_timing_ms": [1], "sent": [{ "t": 1 }] }),
        );
        refused(&["a"], json!({ "sent": [{ "t": 1, "line": "x" }] }));
        refused(&["a", "b"], json!({ "stdout_timing_ms": [5, 1] }));
        refused(
            &[],
            json!({ "sent": [{ "t": 5, "line": "x" }, { "t": 1, "line": "y" }] }),
        );
        refused(&["a"], json!([]));
    }

    #[test]
    fn a_time_past_a_day_is_refused() {
        refused(&["a"], json!({ "stdout_timing_ms": [u64::MAX] }));
        refused(
            &[],
            json!({ "sent": [{ "t": MAX_TIME_MS + 1, "line": "x" }] }),
        );
    }
}
