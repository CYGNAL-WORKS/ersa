//! The app log: Core's own operational records about each session's backend
//! process (started, exited, failed to start, Core restarted), kept beside
//! the database (ADR 0017, ARCH-CORE Observability). They are never part of a
//! session's history; Core turns each into the `session.state` and
//! `session.ended` events it emits itself (PROTO-AGENT-EVENTS-R13).
//!
//! One JSON object per line, appended: `{"ts", "session_id", "type", ...}`,
//! where `type` and the rest are a [`LifecycleFact`]. Core reads the log back
//! once, at startup, for each session's first and last fact: one whose last
//! fact is `process_started` was live when the previous Core ended without
//! recording its exit, and gets `core_restarted`; any other tells how Core
//! last saw the session's backend end, which Core shows when the session is
//! opened.
//!
//! Writes go to one writer thread, so no session's tasks ever wait on the
//! disk. The writer rotates the log when it grows past [`ROTATE_BYTES`],
//! carrying every session's first and last facts into the new file, so
//! rotation never loses what startup reads.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

/// The app log's file name in Core's data directory.
pub const FILE_NAME: &str = "app-log.jsonl";

/// The previous app log, kept once the current one grows past
/// [`ROTATE_BYTES`].
pub const PREVIOUS_FILE_NAME: &str = "app-log.1.jsonl";

/// How large the app log may grow before it is moved aside. Each fact is a
/// short line, so this holds a great many sessions' worth.
const ROTATE_BYTES: u64 = 8 * 1024 * 1024;

/// The `type` of a line carried into a rotated log for a session whose
/// last fact could not be read: not a fact, so its ending stays unknown.
const ENDING_UNKNOWN: &str = "ending_unknown";

/// How long [`AppLog::flushed`] waits for the writer before going on: a disk
/// that hangs must not keep a session from starting.
const FLUSH_WAIT: Duration = Duration::from_secs(5);

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
    /// Its first entry: when Core first started the session's backend.
    pub first: Entry,
    /// Its latest fact; `None` when the latest line about it could not be
    /// read (a newer Core's fact, say), so nothing is known of how it ended.
    pub last: Option<Entry>,
}

/// The app log. Cheap to share: writes are handed to the writer thread.
///
/// A write that fails is reported through tracing and otherwise ignored; a
/// session must not stop because its log could not be written.
pub struct AppLog {
    path: PathBuf,
    /// `None` for a log that writes nowhere.
    writer: Option<mpsc::Sender<Job>>,
}

/// What the writer thread is asked to do.
enum Job {
    Write(Entry),
    /// Stop carrying a removed session's facts into rotated logs.
    Forget(String),
    /// Carry only these sessions' facts: the ones Core still knows.
    Retain(std::collections::HashSet<String>),
    /// Answer once everything sent before is on disk.
    Flush(oneshot::Sender<()>),
}

impl AppLog {
    /// Opens the app log in `data_dir`, and returns it with what it says of
    /// each session, by session id.
    ///
    /// Both the current log and the one moved aside before it are read. A
    /// file that cannot be read is reported and passed over: what it held is
    /// lost, but the log still records from now on. Fails only when the log
    /// cannot be opened for appending.
    pub fn open(data_dir: &Path) -> io::Result<(AppLog, HashMap<String, Facts>)> {
        let path = data_dir.join(FILE_NAME);
        let previous = data_dir.join(PREVIOUS_FILE_NAME);
        let mut facts = HashMap::new();
        for file in [&previous, &path] {
            if let Err(e) = read_into(file, &mut facts) {
                tracing::error!(path = %file.display(), error = %e, "could not read the app log; sessions it would have repaired are not");
            }
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        // A crash mid-write leaves a last line without its end; the next fact
        // must start a line of its own rather than join it and be lost.
        // Should the check fail, the log still records from now on.
        match File::open(&path).and_then(|mut read| ends_with_newline(&mut read)) {
            Ok(true) => {}
            Ok(false) => {
                if let Err(e) = file.write_all(b"\n") {
                    tracing::warn!(path = %path.display(), error = %e, "could not end the app log's torn last line");
                }
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "could not check the app log's last line");
            }
        }
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let (sender, jobs) = mpsc::channel();
        let writer = Writer {
            path: path.clone(),
            previous,
            file: Some(file),
            size,
            rotate_at: ROTATE_BYTES,
            kept: facts
                .iter()
                .map(|(id, f)| (id.clone(), (f.first.clone(), f.last.clone())))
                .collect(),
        };
        std::thread::Builder::new()
            .name("ersa-app-log".into())
            .spawn(move || writer.run(jobs))?;
        Ok((
            AppLog {
                path,
                writer: Some(sender),
            },
            facts,
        ))
    }

    /// An app log that writes nowhere, for a Core whose log could not be
    /// opened: sessions still run, and the failure was reported once.
    pub fn disabled(path: PathBuf) -> AppLog {
        AppLog { path, writer: None }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one fact about session `session_id`, learnt at `ts`. Never
    /// waits: the writer thread writes it.
    pub fn write(&self, session_id: &str, ts: &str, fact: &LifecycleFact) {
        if let Some(writer) = &self.writer {
            let _ = writer.send(Job::Write(Entry {
                ts: ts.to_string(),
                session_id: session_id.to_string(),
                fact: fact.clone(),
            }));
        }
    }

    /// Forgets session `session_id`, which was removed: its facts are not
    /// carried into the log when it next rotates.
    pub fn forget(&self, session_id: &str) {
        if let Some(writer) = &self.writer {
            let _ = writer.send(Job::Forget(session_id.to_string()));
        }
    }

    /// Carries only sessions `known` into the log when it next rotates: the
    /// facts of sessions removed in an earlier run go with it.
    pub fn retain_only(&self, known: std::collections::HashSet<String>) {
        if let Some(writer) = &self.writer {
            let _ = writer.send(Job::Retain(known));
        }
    }

    /// Returns once every fact written before is on disk, or after
    /// [`FLUSH_WAIT`] if the disk has not taken them by then.
    pub async fn flushed(&self) {
        let Some(writer) = &self.writer else {
            return;
        };
        let (done, written) = oneshot::channel();
        if writer.send(Job::Flush(done)).is_err() {
            return;
        }
        if tokio::time::timeout(FLUSH_WAIT, written).await.is_err() {
            tracing::warn!(path = %self.path.display(), "the app log did not take its facts in time");
        }
    }
}

/// The writer thread's state.
struct Writer {
    path: PathBuf,
    previous: PathBuf,
    /// `None` after a rotation that could not reopen the file.
    file: Option<File>,
    size: u64,
    /// The size past which the log is next rotated: further out after a
    /// rotation that failed.
    rotate_at: u64,
    /// Each session's first entry and latest readable one, carried into the
    /// new file when the log rotates.
    kept: HashMap<String, (Entry, Option<Entry>)>,
}

impl Writer {
    /// Writes jobs until every [`AppLog`] is gone.
    fn run(mut self, jobs: mpsc::Receiver<Job>) {
        for job in jobs {
            match job {
                Job::Write(entry) => self.write(entry),
                Job::Forget(session_id) => {
                    self.kept.remove(&session_id);
                }
                Job::Retain(known) => self.kept.retain(|id, _| known.contains(id)),
                Job::Flush(done) => {
                    // To the disk, not only the system's cache.
                    if let Some(file) = &mut self.file
                        && let Err(e) = file.flush().and_then(|()| file.sync_data())
                    {
                        tracing::error!(path = %self.path.display(), error = %e, "could not flush the app log");
                    }
                    let _ = done.send(());
                }
            }
        }
    }

    fn write(&mut self, entry: Entry) {
        self.kept
            .entry(entry.session_id.clone())
            .and_modify(|(_, last)| *last = Some(entry.clone()))
            .or_insert_with(|| (entry.clone(), Some(entry.clone())));
        self.append(&entry);
        if self.size > self.rotate_at {
            self.rotate();
        }
    }

    fn append(&mut self, entry: &Entry) {
        let Some(file) = &mut self.file else {
            return;
        };
        let mut line = serde_json::to_string(entry).expect("app log entries serialize");
        line.push('\n');
        // One `write_all` of the whole line, so a reader never sees two facts
        // interleaved.
        match file.write_all(line.as_bytes()) {
            Ok(()) => self.size += line.len() as u64,
            Err(e) => {
                tracing::error!(path = %self.path.display(), error = %e, session = %entry.session_id, "could not write the app log");
            }
        }
    }

    /// Moves the log aside and starts a new one holding each session's first
    /// and last facts, so what startup reads survives any number of
    /// rotations.
    fn rotate(&mut self) {
        // The new file is written whole beside the old, then swapped in, so a
        // crash midway leaves one or the other complete, never a session's
        // first fact without its last.
        let fresh = self.path.with_extension("jsonl.new");
        let mut carried = String::new();
        let json = |entry: &Entry| serde_json::to_string(entry).expect("app log entries serialize");
        for (id, (first, last)) in &self.kept {
            let mut lines = vec![json(first)];
            match last {
                Some(last) if last != first => lines.push(json(last)),
                Some(_) => {}
                // How it ended is unknown: say so, so its first fact is not
                // taken for its last.
                None => lines.push(
                    serde_json::json!({ "ts": first.ts, "session_id": id, "type": ENDING_UNKNOWN })
                        .to_string(),
                ),
            }
            for line in lines {
                carried.push_str(&line);
                carried.push('\n');
            }
        }
        if let Err(e) = fs::write(&fresh, carried) {
            tracing::warn!(path = %fresh.display(), error = %e, "could not start a new app log; this one grows a while longer");
            self.rotate_at = self.size + ROTATE_BYTES;
            return;
        }
        // Closed first: a file open elsewhere cannot be renamed on Windows.
        self.file = None;
        let swapped =
            fs::rename(&self.path, &self.previous).and_then(|()| fs::rename(&fresh, &self.path));
        match swapped {
            Ok(()) => self.rotate_at = ROTATE_BYTES,
            Err(e) => {
                // Something holds a file open (an indexer, say). Not tried
                // again until the log has grown as much once more, so a
                // failing rename never fills the disk with carried facts.
                tracing::warn!(path = %self.path.display(), error = %e, "could not move the app log aside; it grows a while longer");
                let _ = fs::remove_file(&fresh);
                self.rotate_at = self.size + ROTATE_BYTES;
            }
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(file) => {
                self.file = Some(file);
                self.size = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
            }
            Err(e) => {
                tracing::error!(path = %self.path.display(), error = %e, "could not reopen the app log; facts are not kept from now on");
            }
        }
    }
}

/// Whether `file` is empty or its last byte ends a line.
fn ends_with_newline(file: &mut File) -> io::Result<bool> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::Start(len - 1))?;
    let mut last = [0u8];
    file.read_exact(&mut last)?;
    Ok(last[0] == b'\n')
}

/// Reads one app log file into each session's first and last facts. A
/// missing file is an empty log. A line that names a session but cannot be
/// read as a fact (a newer Core's, say) leaves that session's ending
/// unknown rather than taking an earlier fact for its last.
fn read_into(path: &Path, facts: &mut HashMap<String, Facts>) -> io::Result<()> {
    let mut unreadable = 0;
    for line in lines_in(path)? {
        match serde_json::from_slice::<Entry>(&line) {
            Ok(entry) => {
                facts
                    .entry(entry.session_id.clone())
                    .and_modify(|f| f.last = Some(entry.clone()))
                    .or_insert_with(|| Facts {
                        first: entry.clone(),
                        last: Some(entry),
                    });
            }
            Err(_) => {
                let value = serde_json::from_slice::<Value>(&line).ok();
                if value.as_ref().and_then(|v| v.get("type")?.as_str()) != Some(ENDING_UNKNOWN) {
                    unreadable += 1;
                }
                let named = value.and_then(|v| v.get("session_id")?.as_str().map(String::from));
                if let Some(id) = named
                    && let Some(f) = facts.get_mut(&id)
                {
                    f.last = None;
                }
            }
        }
    }
    if unreadable > 0 {
        tracing::warn!(path = %path.display(), lines = unreadable, "app log lines that are not facts this Core knows were passed over");
    }
    Ok(())
}

/// The lines of one app log file, without their ends; none for a missing
/// file. A read error partway keeps the lines read before it.
fn lines_in(path: &Path) -> io::Result<Vec<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut reader = BufReader::new(file);
    let mut lines = Vec::new();
    loop {
        match next_line(&mut reader) {
            Ok(Some(line)) if line.is_empty() => {}
            Ok(Some(line)) => lines.push(line),
            Ok(None) => break,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "could not read the rest of the app log");
                break;
            }
        }
    }
    Ok(lines)
}

/// The longest app log line read. Core writes short facts; a longer line is
/// not one of them, and is skipped rather than read into memory whole.
const MAX_LINE_BYTES: u64 = 64 * 1024;

/// The next line of `reader` without its terminator, or an empty one for a
/// line longer than [`MAX_LINE_BYTES`], whose rest is skipped. `None` at the
/// end.
fn next_line(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let read = reader
        .by_ref()
        .take(MAX_LINE_BYTES)
        .read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(Some(line));
    }
    if (read as u64) < MAX_LINE_BYTES {
        // The last line, with no end.
        return Ok(Some(line));
    }
    // Too long: skip to its end.
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        match buffer.iter().position(|b| *b == b'\n') {
            Some(end) => {
                reader.consume(end + 1);
                break;
            }
            None => {
                let all = buffer.len();
                reader.consume(all);
            }
        }
    }
    Ok(Some(Vec::new()))
}

/// Every entry of the app log in `data_dir`, oldest first, for tests and
/// diagnostics.
pub fn read_entries(data_dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for name in [PREVIOUS_FILE_NAME, FILE_NAME] {
        for line in lines_in(&data_dir.join(name))? {
            if let Ok(entry) = serde_json::from_slice::<Entry>(&line) {
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
    fn left_live(facts: &HashMap<String, Facts>) -> Vec<&str> {
        let mut live: Vec<&str> = facts
            .iter()
            .filter(|(_, f)| f.last.as_ref().is_some_and(|last| !last.fact.ends()))
            .map(|(id, _)| id.as_str())
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

    /// Opens the log in `dir`, writes `entries`, and waits until they are on
    /// disk.
    async fn write_all(dir: &Path, entries: &[(&str, &str, LifecycleFact)]) {
        let (log, _) = AppLog::open(dir).unwrap();
        for (id, ts, fact) in entries {
            log.write(id, ts, fact);
        }
        log.flushed().await;
    }

    #[tokio::test]
    async fn facts_round_trip_and_a_session_left_started_is_found_live() {
        let dir = tempfile::tempdir().unwrap();
        let exited = LifecycleFact::ProcessExited {
            code: Some(0),
            stopped_by_user: true,
            error: None,
        };
        write_all(
            dir.path(),
            &[
                ("s-1", "2026-11-01T00:00:00.000Z", started()),
                ("s-2", "2026-11-01T00:00:01.000Z", started()),
                ("s-1", "2026-11-01T00:00:02.000Z", exited.clone()),
            ],
        )
        .await;
        // A line cut short by a crash is skipped.
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.path().join(FILE_NAME))
            .unwrap();
        // So is one far too long to be a fact.
        file.write_all(b"\n").unwrap();
        file.write_all(&vec![b'x'; MAX_LINE_BYTES as usize * 2])
            .unwrap();
        file.write_all(b"\n{\"ts\":\"2026").unwrap();
        drop(file);

        let entries = read_entries(dir.path()).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].fact, exited);
        let (log, facts) = AppLog::open(dir.path()).unwrap();
        assert_eq!(left_live(&facts), ["s-2"]);
        assert_eq!(facts["s-1"].last.as_ref().unwrap().fact, exited);
        assert_eq!(facts["s-1"].first.ts, "2026-11-01T00:00:00.000Z");
        // The next fact does not join the cut line, so it is kept.
        log.write(
            "s-2",
            "2026-11-01T00:00:03.000Z",
            &LifecycleFact::CoreRestarted,
        );
        log.flushed().await;
        let (_, facts) = AppLog::open(dir.path()).unwrap();
        assert!(left_live(&facts).is_empty());
    }

    #[tokio::test]
    async fn a_fact_this_core_cannot_read_never_makes_a_session_restarted() {
        let dir = tempfile::tempdir().unwrap();
        write_all(
            dir.path(),
            &[("s-1", "2026-11-01T00:00:00.000Z", started())],
        )
        .await;
        let mut file = OpenOptions::new()
            .append(true)
            .open(dir.path().join(FILE_NAME))
            .unwrap();
        file.write_all(b"{\"ts\":\"2026-11-01T00:00:01.000Z\",\"session_id\":\"s-1\",\"type\":\"from_the_future\"}\n")
            .unwrap();
        drop(file);
        let (_, facts) = AppLog::open(dir.path()).unwrap();
        assert!(left_live(&facts).is_empty());
        assert_eq!(facts["s-1"].last, None);
        assert_eq!(facts["s-1"].first.ts, "2026-11-01T00:00:00.000Z");
    }

    #[tokio::test]
    async fn rotating_while_running_keeps_every_sessions_first_and_last_facts() {
        let dir = tempfile::tempdir().unwrap();
        // A session whose last fact this Core cannot read: its ending stays
        // unknown across rotations, never taken for a session left running.
        fs::write(
            dir.path().join(FILE_NAME),
            "{\"ts\":\"2026-01-01T00:00:00.000Z\",\"session_id\":\"s-new\",\"type\":\"process_started\",\"backend\":\"fake\"}
             {\"ts\":\"2026-01-01T00:00:01.000Z\",\"session_id\":\"s-new\",\"type\":\"from_the_future\"}
",
        )
        .unwrap();
        let (log, _) = AppLog::open(dir.path()).unwrap();
        log.write("s-old", "2026-01-01T00:00:00.000Z", &started());
        log.write("s-gone", "2026-01-01T00:00:00.000Z", &started());
        log.forget("s-gone");
        log.write(
            "s-old",
            "2026-01-01T00:00:01.000Z",
            &LifecycleFact::CoreRestarted,
        );
        // Enough facts about other sessions to rotate twice over.
        let filler = LifecycleFact::StartFailed {
            message: "x".repeat(1024),
            code: None,
            stopped_by_user: false,
        };
        let lines = (ROTATE_BYTES as usize / 1024) * 2 + 10;
        for i in 0..lines {
            log.write(&format!("s-{}", i % 3), "2026-01-02T00:00:00.000Z", &filler);
        }
        log.flushed().await;
        drop(log);
        assert!(dir.path().join(PREVIOUS_FILE_NAME).exists());
        let (_, facts) = AppLog::open(dir.path()).unwrap();
        let old = &facts["s-old"];
        assert_eq!(old.first.ts, "2026-01-01T00:00:00.000Z");
        assert_eq!(
            old.last.as_ref().unwrap().fact,
            LifecycleFact::CoreRestarted
        );
        assert!(left_live(&facts).is_empty());
        assert_eq!(facts["s-new"].last, None);
        // A session removed is not carried into the next rotation.
        let current = fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        assert!(
            !current.contains("s-gone"),
            "the removed session was carried"
        );
        assert_eq!(facts["s-new"].first.ts, "2026-01-01T00:00:00.000Z");
        // Rotated whole: no new file is left half written beside it.
        assert!(!dir.path().join("app-log.jsonl.new").exists());
    }
}
