//! Helpers shared by this crate's integration tests: the recorded fixtures as
//! raw logs, and a sink that stamps lines the way Core's raw log does.

// Each test binary compiles this module and uses a different part of it.
#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use ersa_adapter::recording::Recording;
use ersa_adapter::{AdapterSink, Direction, ProcessExit, Projected, Projector, RawLine};
use ersa_adapter_claude::ClaudeProjector;
use serde_json::Value;
use tokio::sync::watch;

/// One recorded fixture: the CLI's stdout lines and what was written to its stdin.
pub struct Fixture {
    /// `<version>/<file stem>`, such as `2.1.302/01-hello`.
    pub name: String,
    pub stdout: Vec<String>,
    recording: Recording,
}

/// `docs/briefs/protocol/fixtures/claude-code`; the crate sits two levels
/// below the workspace root.
pub fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate sits two levels below the workspace root")
        .join("docs/briefs/protocol/fixtures/claude-code")
}

/// Every fixture of every recorded version, in name order.
pub fn fixtures() -> Vec<Fixture> {
    let mut found = Vec::new();
    for version in fs::read_dir(fixtures_root()).expect("fixtures directory exists") {
        let version = version.unwrap().path();
        if !version.is_dir() {
            continue;
        }
        let version_name = version.file_name().unwrap().to_string_lossy().into_owned();
        for file in fs::read_dir(&version).unwrap() {
            let file = file.unwrap().path();
            let Some(stem) = file
                .file_name()
                .unwrap()
                .to_str()
                .and_then(|n| n.strip_suffix(".jsonl"))
            else {
                continue;
            };
            let name = format!("{version_name}/{stem}");
            let recording = Recording::load(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
            found.push(Fixture {
                name,
                stdout: recording.stdout(),
                recording,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    assert!(!found.is_empty(), "no fixtures found");
    found
}

pub fn fixture(name: &str) -> Fixture {
    fixtures()
        .into_iter()
        .find(|f| f.name.ends_with(name))
        .unwrap_or_else(|| panic!("no fixture {name}"))
}

impl Fixture {
    /// The lines written to stdin, in order.
    pub fn sent(&self) -> Vec<String> {
        self.recording.sent()
    }

    /// The session's raw log as Core would have recorded it (see
    /// [`Recording::lines`] for the order).
    pub fn raw_log(&self) -> Vec<RawLine> {
        self.recording.raw_log()
    }
}

/// Runs a fresh projector over `lines`, as Core does on each projection pass,
/// pairing each event with the index of the line that completed it (the
/// envelope's `raw_to`).
pub fn project(lines: &[RawLine]) -> Vec<(u64, Projected)> {
    let mut projector = ClaudeProjector::new();
    lines
        .iter()
        .flat_map(|line| {
            projector
                .project(line)
                .into_iter()
                .map(|projected| (line.index, projected))
        })
        .collect()
}

/// Just the events of a projection.
pub fn events(lines: &[RawLine]) -> Vec<ersa_protocol::events::Event> {
    project(lines).into_iter().map(|(_, p)| p.event).collect()
}

/// A raw log from `(direction, line)` pairs, for hand-written cases.
pub fn log(entries: &[(Direction, &str)]) -> Vec<RawLine> {
    entries
        .iter()
        .enumerate()
        .map(|(index, (dir, line))| RawLine {
            index: index as u64,
            ts: "2026-10-31T12:00:00Z".into(),
            dir: *dir,
            line: line.to_string(),
        })
        .collect()
}

/// Core's side for tests: stamps each recorded line with the next index, and
/// lets a test wait until some line has arrived.
pub struct RecordingSink {
    lines: Mutex<Vec<RawLine>>,
    exits: Mutex<Vec<io::Result<ProcessExit>>>,
    stderr: Mutex<Vec<String>>,
    /// Bumped on every record and exit, so waiters wake to re-check.
    changed: watch::Sender<u64>,
}

impl RecordingSink {
    pub fn new() -> Self {
        RecordingSink {
            lines: Mutex::default(),
            exits: Mutex::default(),
            stderr: Mutex::default(),
            changed: watch::channel(0).0,
        }
    }

    pub fn lines(&self) -> Vec<RawLine> {
        self.lines.lock().unwrap().clone()
    }

    pub fn exit_codes(&self) -> Vec<Option<i32>> {
        self.exits
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.as_ref().expect("exit").code)
            .collect()
    }

    pub fn stderr(&self) -> Vec<String> {
        self.stderr.lock().unwrap().clone()
    }

    /// Waits until a recorded line matches `wanted` and returns it, failing
    /// the test after `timeout`.
    pub async fn wait_for_line(
        &self,
        timeout: Duration,
        wanted: impl Fn(&RawLine) -> bool,
    ) -> RawLine {
        let mut changed = self.changed.subscribe();
        let found = tokio::time::timeout(timeout, async {
            loop {
                if let Some(line) = self.lines().into_iter().find(|l| wanted(l)) {
                    return line;
                }
                changed.changed().await.expect("sink alive");
            }
        })
        .await;
        found.unwrap_or_else(|_| {
            panic!(
                "no matching line within {timeout:?}; log: {:#?}",
                self.lines()
            )
        })
    }

    /// Waits until the process exit has been reported.
    pub async fn wait_for_exit(&self, timeout: Duration) {
        let mut changed = self.changed.subscribe();
        let exited = tokio::time::timeout(timeout, async {
            while self.exits.lock().unwrap().is_empty() {
                changed.changed().await.expect("sink alive");
            }
        })
        .await;
        exited.unwrap_or_else(|_| panic!("no exit within {timeout:?}"));
    }
}

impl AdapterSink for RecordingSink {
    fn record(&self, dir: Direction, line: String) {
        let mut lines = self.lines.lock().unwrap();
        let index = lines.len() as u64;
        lines.push(RawLine {
            index,
            // A fixed time keeps projections deterministic; Core uses the clock.
            ts: "2026-10-31T12:00:00Z".into(),
            dir,
            line,
        });
        drop(lines);
        self.changed.send_modify(|n| *n += 1);
    }

    fn stderr_line(&self, line: String) {
        self.stderr.lock().unwrap().push(line);
    }

    fn process_exited(&self, exit: io::Result<ProcessExit>) {
        self.exits.lock().unwrap().push(exit);
        self.changed.send_modify(|n| *n += 1);
    }
}

/// Whether a raw line is a JSON object of the given `type` (and, for control
/// requests, `request.subtype`).
pub fn is_kind(line: &RawLine, kind: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(&line.line) else {
        return false;
    };
    match kind.split_once('/') {
        Some(("control_request", subtype)) => {
            value["type"] == "control_request" && value["request"]["subtype"] == subtype
        }
        Some((ty, subtype)) => value["type"] == ty && value["subtype"] == subtype,
        None => value["type"] == kind,
    }
}
