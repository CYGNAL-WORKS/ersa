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
//! [`Recording::timed_lines`].

use std::fs;
use std::io;
use std::path::Path;

use serde_json::Value;

use crate::raw::{Direction, RawLine};

/// One recorded session.
#[derive(Debug, Clone)]
pub struct Recording {
    /// The backend's stdout lines, blank lines dropped.
    pub stdout: Vec<String>,
    /// The recorder's notes, verbatim: backend-specific keys (such as the
    /// backend's version) are read from here by whoever knows them.
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
        let text = fs::read_to_string(jsonl)?;
        let meta_path = jsonl.with_file_name(format!("{stem}.meta.json"));
        let meta = fs::read_to_string(&meta_path)?;
        let meta = serde_json::from_str(&meta).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", meta_path.display()),
            )
        })?;
        let recording = Recording {
            stdout: text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(String::from)
                .collect(),
            meta,
        };
        // Checked here, so a bad file fails when it is loaded, not halfway
        // through a replay.
        recording.timed_lines()?;
        Ok(recording)
    }

    /// When the recording was made, RFC 3339, if it says.
    pub fn recorded_at(&self) -> Option<&str> {
        self.meta["recorded_at"].as_str()
    }

    /// The lines written to the backend's stdin, in order.
    pub fn sent(&self) -> Vec<String> {
        self.sent_with_times()
            .unwrap_or_default()
            .into_iter()
            .map(|(_, line)| line)
            .collect()
    }

    fn sent_with_times(&self) -> io::Result<Vec<(u64, String)>> {
        let Some(sent) = self.meta["sent"].as_array() else {
            return Ok(Vec::new());
        };
        sent.iter()
            .map(|s| match (s["t"].as_u64(), s["line"].as_str()) {
                (Some(t), Some(line)) => Ok((t, line.to_string())),
                _ => Err(invalid("every `sent` entry needs a `t` and a `line`")),
            })
            .collect()
    }

    /// Every line that crossed the backend's stdio, in the order Core would
    /// have recorded them, each with the milliseconds since the start at
    /// which it crossed.
    ///
    /// Stdout and stdin lines are interleaved by the times the recording
    /// noted. A stdout line and a stdin line at the same millisecond go
    /// stdout first, since the host writes in reaction to what it read. A
    /// one-shot recording has no timings and no stdin lines: its lines are
    /// the stdout lines in order, all at 0.
    pub fn timed_lines(&self) -> io::Result<Vec<(u64, Direction, String)>> {
        let stdout_times: Vec<u64> = match self.meta["stdout_timing_ms"].as_array() {
            Some(times) => times
                .iter()
                .map(|t| {
                    t.as_u64()
                        .ok_or_else(|| invalid("a stdout timing is not a number"))
                })
                .collect::<io::Result<_>>()?,
            None => vec![0; self.stdout.len()],
        };
        if stdout_times.len() != self.stdout.len() {
            return Err(invalid(&format!(
                "{} stdout timings for {} stdout lines",
                stdout_times.len(),
                self.stdout.len()
            )));
        }
        let mut sent = self.sent_with_times()?.into_iter().peekable();
        let mut lines = Vec::with_capacity(self.stdout.len());
        let mut out = 0;
        while out < self.stdout.len() || sent.peek().is_some() {
            let take_stdout = match sent.peek() {
                Some((t, _)) => out < self.stdout.len() && stdout_times[out] <= *t,
                None => true,
            };
            if take_stdout {
                lines.push((
                    stdout_times[out],
                    Direction::Backend,
                    self.stdout[out].clone(),
                ));
                out += 1;
            } else if let Some((t, line)) = sent.next() {
                lines.push((t, Direction::Host, line));
            }
        }
        Ok(lines)
    }

    /// The session's raw log as Core would have recorded it, every line
    /// stamped with the recording's time.
    pub fn raw_log(&self) -> io::Result<Vec<RawLine>> {
        let ts = self.recorded_at().unwrap_or("2026-10-31T00:00:00Z");
        Ok(self
            .timed_lines()?
            .into_iter()
            .enumerate()
            .map(|(index, (_, dir, line))| RawLine {
                index: index as u64,
                ts: ts.to_string(),
                dir,
                line,
            })
            .collect())
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn recording(stdout: &[&str], meta: Value) -> Recording {
        Recording {
            stdout: stdout.iter().map(|s| s.to_string()).collect(),
            meta,
        }
    }

    #[test]
    fn lines_interleave_by_time_with_stdout_first_on_a_tie() {
        let r = recording(
            &["a", "b", "c"],
            json!({
                "stdout_timing_ms": [10, 20, 30],
                "sent": [{ "t": 5, "line": "x" }, { "t": 20, "line": "y" }]
            }),
        );
        let lines = r.timed_lines().unwrap();
        let order: Vec<(u64, &str)> = lines.iter().map(|(t, _, l)| (*t, l.as_str())).collect();
        assert_eq!(
            order,
            [(5, "x"), (10, "a"), (20, "b"), (20, "y"), (30, "c")]
        );
        assert_eq!(lines[0].1, Direction::Host);
        assert_eq!(lines[1].1, Direction::Backend);
    }

    #[test]
    fn a_one_shot_recording_is_its_stdout_at_zero() {
        let r = recording(&["a", "b"], json!({}));
        let lines = r.timed_lines().unwrap();
        assert_eq!(
            lines,
            [
                (0, Direction::Backend, "a".to_string()),
                (0, Direction::Backend, "b".to_string())
            ]
        );
    }

    #[test]
    fn timings_that_do_not_match_the_lines_are_an_error() {
        let r = recording(&["a", "b"], json!({ "stdout_timing_ms": [1] }));
        assert_eq!(
            r.timed_lines().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
