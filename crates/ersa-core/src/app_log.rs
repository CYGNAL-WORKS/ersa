//! The app log: Core's own operational records about each session's backend
//! process (started, exited, failed to start, Core restarted), kept beside
//! the database (ADR 0017, ARCH-CORE Observability). They are never part of a
//! session's history; Core turns each into the `session.state` and
//! `session.ended` events it emits itself (PROTO-AGENT-EVENTS-R13).
//!
//! One JSON object per line, appended: `{"ts", "session_id", "type", ...}`,
//! where `type` and the rest are a [`LifecycleFact`]. Core reads the log back
//! once, at startup, for each session's last fact: one whose last fact is
//! `process_started` was live when the previous Core ended without recording
//! its exit, and gets `core_restarted`; any other tells how Core last saw
//! the session's backend end, which Core shows when the session is opened.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The app log's file name in Core's data directory.
pub const FILE_NAME: &str = "app-log.jsonl";

/// The previous app log, kept once the current one grows past
/// [`ROTATE_BYTES`].
pub const PREVIOUS_FILE_NAME: &str = "app-log.1.jsonl";

/// How large the app log may grow before startup moves it aside. Each fact
/// is a short line, so this holds a great many sessions' worth.
const ROTATE_BYTES: u64 = 8 * 1024 * 1024;

/// What Core knows about a session's backend process.
///
/// A small JSON shape of Ersa's own, tagged by `type`, such as
/// `{"type":"process_exited","code":0}`. Fields are only ever added, with
/// defaults, so every fact ever written keeps parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LifecycleFact {
    /// Core is starting the backend process.
    ProcessStarted {
        backend: String,
        /// The version detection found, or empty when unknown. The backend's
        /// own announcement (`session.started`) replaces it.
        #[serde(default)]
        backend_version: String,
    },
    /// The backend process ended.
    ProcessExited {
        /// The exit code; absent when there was none (killed) or the
        /// transport lost track of the process.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        /// The user had asked for the session to stop.
        #[serde(default, skip_serializing_if = "is_false")]
        stopped_by_user: bool,
        /// Why the exit is unknown, when the transport lost track of it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// The adapter did not finish starting the backend. Ends the session;
    /// when the process had already exited, this fact carries its code
    /// instead of a `process_exited` of its own.
    StartFailed {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        /// The user stopped the session before it was ready, which is a
        /// stop, not a failure.
        #[serde(default, skip_serializing_if = "is_false")]
        stopped_by_user: bool,
    },
    /// Core found the session live when it started: the Core that ran it
    /// ended without recording the backend's exit.
    CoreRestarted,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl LifecycleFact {
    /// Whether the fact ends the session's backend process.
    pub fn ends(&self) -> bool {
        !matches!(self, LifecycleFact::ProcessStarted { .. })
    }
}

/// One line of the app log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// When Core learnt the fact, RFC 3339 UTC.
    pub ts: String,
    pub session_id: String,
    #[serde(flatten)]
    pub fact: LifecycleFact,
}

/// What the app log says about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// When Core first learnt anything about it: when it first started the
    /// session's backend.
    pub first_ts: String,
    /// Its latest fact.
    pub last: Entry,
}

/// The app log, open for appending.
///
/// Writes are one short line each, under a lock, straight to the file: a
/// fact must be on disk before a crash for startup to repair what the crash
/// left. A write that fails is reported through tracing and otherwise
/// ignored; a session must not stop because its log could not be written.
pub struct AppLog {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl AppLog {
    /// Opens the app log in `data_dir`, and returns it with what it says of
    /// each session, by session id. A session whose last fact is
    /// `process_started` was live when the log was last written; one whose
    /// last fact ends it tells how Core last saw its backend end.
    ///
    /// A log past [`ROTATE_BYTES`] is read, then moved aside for an empty
    /// one; its predecessor is read too, so what happened just before a
    /// rotation is still found.
    pub fn open(data_dir: &Path) -> io::Result<(AppLog, HashMap<String, Facts>)> {
        let path = data_dir.join(FILE_NAME);
        let previous = data_dir.join(PREVIOUS_FILE_NAME);
        let mut last = HashMap::new();
        for file in [&previous, &path] {
            read_into(file, &mut last)?;
        }
        if fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_BYTES)
            && let Err(e) = fs::rename(&path, &previous)
        {
            // Another process may hold it open; it grows a while longer.
            tracing::warn!(path = %path.display(), error = %e, "could not move the app log aside");
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok((
            AppLog {
                path,
                file: Mutex::new(Some(file)),
            },
            last,
        ))
    }

    /// An app log that writes nowhere, for a Core whose log could not be
    /// opened: sessions still run, and the failure was reported once.
    pub fn disabled(path: PathBuf) -> AppLog {
        AppLog {
            path,
            file: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one fact about session `session_id`, learnt at `ts`.
    pub fn write(&self, session_id: &str, ts: &str, fact: &LifecycleFact) {
        let entry = Entry {
            ts: ts.to_string(),
            session_id: session_id.to_string(),
            fact: fact.clone(),
        };
        let mut line = serde_json::to_string(&entry).expect("app log entries serialize");
        line.push('\n');
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let Some(file) = file.as_mut() else {
            return;
        };
        // One `write_all` of the whole line: appends from one process land
        // whole, so a reader never sees two facts interleaved.
        if let Err(e) = file.write_all(line.as_bytes()) {
            tracing::error!(path = %self.path.display(), error = %e, session = session_id, "could not write the app log");
        }
    }
}

/// Reads one app log file, keeping each session's first time and last entry.
/// A missing file is an empty log; a line that is not a fact (a newer Core's,
/// or one cut short by a crash) is skipped.
fn read_into(path: &Path, last: &mut HashMap<String, Facts>) -> io::Result<()> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for line in BufReader::new(file).split(b'\n') {
        let line = line?;
        let Ok(entry) = serde_json::from_slice::<Entry>(&line) else {
            continue;
        };
        last.entry(entry.session_id.clone())
            .and_modify(|facts: &mut Facts| facts.last = entry.clone())
            .or_insert_with(|| Facts {
                first_ts: entry.ts.clone(),
                last: entry,
            });
    }
    Ok(())
}

// Every entry of the app log in `data_dir`, oldest first, for tests and
/// diagnostics.
pub fn read_entries(data_dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for name in [PREVIOUS_FILE_NAME, FILE_NAME] {
        let file = match File::open(data_dir.join(name)) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        for line in BufReader::new(file).lines() {
            if let Ok(entry) = serde_json::from_str::<Entry>(&line?) {
                entries.push(entry);
            }
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sessions whose last fact leaves them live, by id.
    fn left_live(last: &HashMap<String, Facts>) -> Vec<&str> {
        let mut live: Vec<&str> = last
            .values()
            .filter(|f| !f.last.fact.ends())
            .map(|f| f.last.session_id.as_str())
            .collect();
        live.sort();
        live
    }

    fn started() -> LifecycleFact {
        LifecycleFact::ProcessStarted {
            backend: "fake".into(),
            backend_version: "1.0".into(),
        }
    }

    #[test]
    fn facts_round_trip_and_a_session_left_started_is_found_live() {
        let dir = tempfile::tempdir().unwrap();
        let (log, live) = AppLog::open(dir.path()).unwrap();
        assert!(live.is_empty());
        let exited = LifecycleFact::ProcessExited {
            code: Some(0),
            stopped_by_user: true,
            error: None,
        };
        log.write("s-1", "2026-11-01T00:00:00.000Z", &started());
        log.write("s-2", "2026-11-01T00:00:01.000Z", &started());
        log.write("s-1", "2026-11-01T00:00:02.000Z", &exited);
        drop(log);
        // A line cut short by a crash is skipped.
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.path().join(FILE_NAME))
            .unwrap();
        file.write_all(b"{\"ts\":\"2026").unwrap();
        drop(file);

        let entries = read_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].fact, exited);
        let (_, live) = AppLog::open(dir.path()).unwrap();
        assert_eq!(left_live(&live), ["s-2"]);
        assert_eq!(live["s-1"].last.fact, exited);
        assert_eq!(live["s-1"].first_ts, "2026-11-01T00:00:00.000Z");
    }

    #[test]
    fn a_large_log_is_moved_aside_and_still_read() {
        let dir = tempfile::tempdir().unwrap();
        let line = serde_json::to_string(&Entry {
            ts: "2026-11-01T00:00:00.000Z".into(),
            session_id: "s-old".into(),
            fact: started(),
        })
        .unwrap();
        let big = format!("{line}\n").repeat((ROTATE_BYTES as usize / line.len()) + 2);
        fs::write(dir.path().join(FILE_NAME), big).unwrap();
        let (log, live) = AppLog::open(dir.path()).unwrap();
        assert_eq!(left_live(&live), ["s-old"]);
        log.write(
            "s-old",
            "2026-11-01T00:00:01.000Z",
            &LifecycleFact::CoreRestarted,
        );
        drop(log);
        assert!(dir.path().join(PREVIOUS_FILE_NAME).exists());
        let (_, live) = AppLog::open(dir.path()).unwrap();
        assert!(left_live(&live).is_empty());
    }
}
