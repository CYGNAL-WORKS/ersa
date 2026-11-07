//! Claude Code's own session record, which is a session's history (ADR 0017,
//! PROTO-CLAUDE-CODE-STREAM, Session record).
//!
//! The CLI appends each session to
//! `<claude home>/projects/<encoded cwd>/<session id>.jsonl` as it runs,
//! including turns taken outside Ersa, and lists the processes that have a
//! session open in `<claude home>/sessions/<pid>.json`. `<claude home>` is
//! normally `~/.claude` on the session's host; callers pass it in.
//!
//! - [`project_record`] turns a record into Ersa events, through the
//!   same projection of messages as the live stream, and [`read_record`]
//!   finds a session's record and does that to it.
//! - [`list_sessions`] lists a directory's sessions, reading only the head and
//!   tail of each record.
//! - [`live_session`] says whether a running CLI has a session open.
//! - [`append_title`] renames a session no CLI of Ersa's has open, and
//!   [`delete_record`] deletes one for the user's Delete: the only writes
//!   Ersa makes to the CLI's files.
//!
//! These read local files. A remote host's record needs reading through its
//! transport, which has no file access yet.

mod listing;
mod projection;
mod registry;

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use ersa_protocol::events::{Event, MessageAttachment, UserMessage};

pub use listing::{SessionList, SessionSummary, list_sessions, summarize_session};
pub use projection::{RecordEvent, project_record, project_record_from};
pub use registry::{LiveCheck, LiveSession, live_session};

/// The longest encoded directory name the CLI uses whole; a longer one is cut
/// and given a hash of the full path. Verified on 2.1.316: a 222-character
/// working directory was recorded under its first 200 encoded characters,
/// `-`, and exactly the hash [`path_hash`] computes.
const MAX_ENCODED_LEN: usize = 200;

/// The name of the directory under `projects/` that holds the sessions of
/// working directory `cwd`: every character other than an ASCII letter or
/// digit becomes `-` (`C:\Users\user\ersa` is `C--Users-user-ersa`).
///
/// The CLI is JavaScript and counts in UTF-16 code units, so a character
/// outside the Basic Multilingual Plane (an emoji, say) becomes two dashes.
pub fn encoded_cwd(cwd: &str) -> String {
    let mut encoded = String::with_capacity(cwd.len());
    for c in cwd.chars() {
        if c.is_ascii_alphanumeric() {
            encoded.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                encoded.push('-');
            }
        }
    }
    if encoded.len() <= MAX_ENCODED_LEN {
        return encoded;
    }
    // Every character is ASCII by now, so cutting at a byte index is safe.
    encoded.truncate(MAX_ENCODED_LEN);
    format!("{encoded}-{}", path_hash(cwd))
}

/// The hash the CLI appends to a cut directory name: the 32-bit string hash
/// JavaScript code commonly uses (`h = h * 31 + unit`, over UTF-16 code units,
/// wrapping), its absolute value in base 36.
fn path_hash(cwd: &str) -> String {
    let hash = cwd.encode_utf16().fold(0i32, |h, unit| {
        h.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    // `unsigned_abs` keeps i32::MIN's magnitude, as JavaScript's `Math.abs`
    // does on its doubles.
    let mut n = hash.unsigned_abs();
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(char::from_digit(n % 36, 36).expect("below 36"));
        n /= 36;
    }
    digits.iter().rev().collect()
}

/// The directory holding the session records of working directory `cwd`.
///
/// `cwd` must be spelled as the CLI spelled it, which is the `cwd` its
/// `system/init` reports: a canonicalised path (`\?\D:\...`), another
/// drive-letter case or a trailing separator encodes to another directory,
/// and that directory simply has no sessions.
pub fn project_dir(claude_home: &Path, cwd: &str) -> PathBuf {
    claude_home.join("projects").join(encoded_cwd(cwd))
}

/// The session record of session `session_id`, started in `cwd`.
pub fn record_path(claude_home: &Path, cwd: &str, session_id: &str) -> PathBuf {
    project_dir(claude_home, cwd).join(format!("{session_id}.jsonl"))
}

/// The longest record [`read_record`] reads. Records of long sessions with
/// large tool results run to tens of megabytes; one far beyond is not
/// something a transcript can show, and projecting it costs Core several
/// times its size in memory while it is opened.
pub const MAX_RECORD_BYTES: u64 = 256 * 1024 * 1024;

/// Where the CLI keeps its files on this machine, as the CLI itself decides:
/// `CLAUDE_CONFIG_DIR` when set and not empty, else `.claude` in the user's
/// home directory. `None` when there is no home directory either.
///
/// Read from Core's own environment, which the CLI inherits: the adapter
/// does not remove `CLAUDE_CONFIG_DIR`, so the CLI writes where this reads.
pub fn default_claude_home() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => std::env::home_dir().map(|home| home.join(".claude")),
    }
}

/// The file holding session `session_id`'s record: under `cwd`'s directory
/// when it is there, else under whichever project directory has it. `cwd`
/// may be spelled differently from the CLI's own spelling (a canonicalised
/// path, another drive-letter case), and a session id names one session
/// wherever it ran, so the search is what finds it then. `None` when no
/// record exists: never written yet, or deleted by the CLI's retention.
pub fn find_record(claude_home: &Path, cwd: &str, session_id: &str) -> io::Result<Option<PathBuf>> {
    if !is_session_id(session_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{session_id:?} is not a Claude Code session id"),
        ));
    }
    let expected = record_path(claude_home, cwd, session_id);
    if is_plain_file(&expected) {
        return Ok(Some(expected));
    }
    let projects = match fs::read_dir(claude_home.join("projects")) {
        Ok(projects) => projects,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let file = format!("{session_id}.jsonl");
    for project in projects {
        let candidate = project?.path().join(&file);
        if is_plain_file(&candidate) {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

/// Whether `path` is a file itself, not a link to one: the CLI writes its
/// records as plain files, and listing skips links too.
fn is_plain_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}

/// The events of session `session_id`'s record ([`project_record`]), or none
/// when it has no record ([`find_record`]). Read line by line; bytes that are
/// not UTF-8 are read with replacement characters, as Core reads a backend's
/// lines. Only the bytes the file holds when it is opened are read: the CLI
/// may be appending to it.
pub fn read_record(
    claude_home: &Path,
    cwd: &str,
    session_id: &str,
) -> io::Result<Vec<RecordEvent>> {
    match find_record(claude_home, cwd, session_id)? {
        Some(path) => Ok(read_record_at(&path)?.events),
        None => Ok(Vec::new()),
    }
}

/// A token for the state of session `session_id`'s record: its file's size
/// and modified time. `None` when it has no record, or its file system
/// keeps no modified times.
pub fn record_version(
    claude_home: &Path,
    cwd: &str,
    session_id: &str,
) -> io::Result<Option<String>> {
    let Some(path) = find_record(claude_home, cwd, session_id)? else {
        return Ok(None);
    };
    let metadata = fs::metadata(&path)?;
    let Ok(modified) = metadata.modified() else {
        return Ok(None);
    };
    let since = modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(Some(format!("{}:{}", metadata.len(), since.as_nanos())))
}

/// Deletes session `session_id`'s record, for the user's Delete
/// (PROTO-CLAUDE-CODE-STREAM, Deleting): its record file and the folder of
/// the same name beside it, which holds the session's subagent transcripts
/// and stored tool results, and nothing else of the CLI's. A record already
/// gone counts as deleted, and its folder, if left under `cwd`'s directory,
/// goes too.
///
/// The folder goes first and the file last, so a deletion that fails part
/// way leaves the file, and a second try finds the folder beside it again,
/// wherever the record was filed. Only a real folder is deleted: a link of
/// that name is not the CLI's to follow. Checking that no other process has
/// the session open is the caller's.
pub fn delete_record(claude_home: &Path, cwd: &str, session_id: &str) -> io::Result<()> {
    let file = find_record(claude_home, cwd, session_id)?;
    // `find_record` checked the id, so it is a plain name, never a path.
    let folder = match &file {
        Some(file) => file.with_file_name(session_id),
        None => project_dir(claude_home, cwd).join(session_id),
    };
    if fs::symlink_metadata(&folder).is_ok_and(|m| m.file_type().is_dir()) {
        match fs::remove_dir_all(&folder) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    if let Some(file) = file {
        match fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

/// Renames session `session_id` by appending one `custom-title` entry to its
/// record, as the vendor's IDE extension does for a session no CLI has open
/// (PROTO-CLAUDE-CODE-STREAM, Renaming). This is the only write Ersa makes to
/// a session record: everything already in the file stays as it is.
///
/// The entry goes out in a single write. A record whose last line has no
/// newline is being written by a CLI right now (the CLI ends every line it
/// writes), or was left so by one that crashed: Ersa does not guess which,
/// and refuses with `ResourceBusy`. A write the disk takes only part of is
/// ended with a newline, never cut back (cutting could take a CLI's line with
/// it), so the CLI's next line is not glued onto it; the torn part reads as
/// an unreadable line. A session with no record is a `NotFound` error, and
/// nothing is created.
pub fn append_title(
    claude_home: &Path,
    cwd: &str,
    session_id: &str,
    title: &str,
) -> io::Result<()> {
    let Some(path) = find_record(claude_home, cwd, session_id)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("session {session_id} has no record to rename"),
        ));
    };
    let entry = crate::wire::RecordEntry::CustomTitle(crate::wire::CustomTitle {
        custom_title: title.to_string(),
        session_id: Some(session_id.to_string()),
        extra: Default::default(),
    });
    // JSON escapes any line break in the title, so the entry is one line.
    let mut line = serde_json::to_string(&entry)?;
    line.push('\n');

    append_line(&mut RecordFile::open(path)?, line.as_bytes())
}

/// Where [`append_line`] appends: a record file, or a stand-in in tests.
trait AppendTarget {
    /// The file's length now.
    fn len(&mut self) -> io::Result<u64>;
    /// Its last byte, when it has one.
    fn last_byte(&mut self) -> io::Result<Option<u8>>;
    /// One write at the end of the file; how much of `bytes` it took.
    fn append(&mut self, bytes: &[u8]) -> io::Result<usize>;
}

/// Appends `line` (ending in a newline) to `target` as one write, refusing a
/// target whose last line is unterminated, and ending a partial write.
fn append_line(target: &mut impl AppendTarget, line: &[u8]) -> io::Result<()> {
    if target.last_byte()?.is_some_and(|last| last != b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "Claude Code is still writing this session's record, or did not finish it; \
             resuming the session once in Claude Code completes it, and the rename can be made then",
        ));
    }
    // One write, not a loop: a loop that fails halfway leaves its first part
    // behind with no way to tell it from a whole line.
    let written = match target.append(line) {
        Ok(written) if written == line.len() => return Ok(()),
        Ok(written) => written,
        Err(e) if e.kind() == io::ErrorKind::Interrupted => 0,
        Err(e) => return Err(e),
    };
    // Best effort: end the torn part, so it is a line of its own that
    // readers skip as unreadable, and the CLI's next line stays whole.
    if written > 0 {
        let _ = target.append(b"\n");
    }
    Err(io::Error::new(
        io::ErrorKind::WriteZero,
        format!(
            "the disk took {written} of the entry's {} bytes; the rename was not made",
            line.len()
        ),
    ))
}

/// A session record opened to append to.
struct RecordFile {
    file: fs::File,
}

impl RecordFile {
    fn open(path: PathBuf) -> io::Result<Self> {
        // Opened for reading too, to look at the last byte; appends always
        // go to the end, wherever the cursor is.
        let file = fs::OpenOptions::new().read(true).append(true).open(&path)?;
        Ok(RecordFile { file })
    }
}

impl AppendTarget for RecordFile {
    fn len(&mut self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    fn last_byte(&mut self) -> io::Result<Option<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let len = self.len()?;
        if len == 0 {
            return Ok(None);
        }
        let mut last = [0u8];
        self.file.seek(SeekFrom::Start(len - 1))?;
        self.file.read_exact(&mut last)?;
        Ok(Some(last[0]))
    }

    fn append(&mut self, bytes: &[u8]) -> io::Result<usize> {
        use std::io::Write;
        self.file.write(bytes)
    }
}

/// A record's events, with the size and modified time of the file they were
/// read from.
#[derive(Debug, Clone)]
struct ReadRecord {
    len: u64,
    modified: Option<SystemTime>,
    events: Vec<RecordEvent>,
}

fn read_record_at(path: &Path) -> io::Result<ReadRecord> {
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    let len = metadata.len();
    if len > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "the session record {} is {len} bytes, more than the {MAX_RECORD_BYTES} Ersa reads",
                path.display()
            ),
        ));
    }
    let mut reader = io::BufReader::new(file);
    Ok(ReadRecord {
        len,
        modified: metadata.modified().ok(),
        events: projection::project_record_from(&mut reader, len)?,
    })
}

/// The most bytes of projected events [`RecordCache`] keeps across all the
/// records it holds: the same budget as one open view in Core.
const CACHED_RECORD_BYTES: usize = 64 * 1024 * 1024;

/// How many summaries [`RecordCache`] keeps; each is a few hundred bytes.
const CACHED_SUMMARIES: usize = 256;

/// One record's projection, kept while its file is unchanged.
#[derive(Debug)]
struct CachedRecord {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
    events: Arc<Vec<RecordEvent>>,
    /// About how much memory `events` holds.
    bytes: usize,
}

/// The projections of the records read last, each kept for as long as its
/// file's size and modified time say it has not changed, so opening a large
/// unchanged record again costs a look at its metadata, not a read. A record
/// that changed (the CLI appended a turn) is read again. Bounded by
/// [`CACHED_RECORD_BYTES`] in all; results are shared, never copied.
#[derive(Debug, Default)]
pub struct RecordCache {
    /// Most recently used last.
    records: Mutex<VecDeque<CachedRecord>>,
    /// Summaries of the records summarized last, by the same key. A summary
    /// that took long to read is still there for the next caller while the
    /// record is unchanged.
    summaries: Mutex<VecDeque<(PathBuf, u64, SystemTime, SessionSummary)>>,
    /// How many times a record file was read and projected.
    reads: AtomicUsize,
}

impl RecordCache {
    /// [`read_record`], through the cache.
    pub fn read(
        &self,
        claude_home: &Path,
        cwd: &str,
        session_id: &str,
    ) -> io::Result<Arc<Vec<RecordEvent>>> {
        let Some(path) = find_record(claude_home, cwd, session_id)? else {
            return Ok(Arc::default());
        };
        let metadata = fs::metadata(&path)?;
        let (len, modified) = (metadata.len(), metadata.modified().ok());
        if let Some(modified) = modified {
            let mut records = lock(&self.records);
            if let Some(at) = records
                .iter()
                .position(|r| r.path == path && r.len == len && r.modified == modified)
            {
                let hit = records.remove(at).expect("found");
                let events = Arc::clone(&hit.events);
                records.push_back(hit);
                return Ok(events);
            }
        }
        self.reads.fetch_add(1, Ordering::SeqCst);
        let read = read_record_at(&path)?;
        let events = Arc::new(read.events);
        // Keyed by what the file was when it was opened for this read.
        if let Some(modified) = read.modified {
            let bytes = approximate_bytes(&events);
            let mut records = lock(&self.records);
            records.retain(|r| r.path != path);
            if bytes <= CACHED_RECORD_BYTES {
                records.push_back(CachedRecord {
                    path,
                    len: read.len,
                    modified,
                    events: Arc::clone(&events),
                    bytes,
                });
                while records.iter().map(|r| r.bytes).sum::<usize>() > CACHED_RECORD_BYTES {
                    records.pop_front();
                }
            }
        }
        Ok(events)
    }

    /// [`summarize_session`], through the cache.
    pub fn summarize(
        &self,
        claude_home: &Path,
        cwd: &str,
        session_id: &str,
    ) -> io::Result<Option<SessionSummary>> {
        let Some(path) = find_record(claude_home, cwd, session_id)? else {
            return Ok(None);
        };
        let metadata = fs::metadata(&path)?;
        let len = metadata.len();
        let Ok(modified) = metadata.modified() else {
            return summarize_session(claude_home, cwd, session_id);
        };
        if let Some((.., summary)) = lock(&self.summaries)
            .iter()
            .find(|(p, l, m, _)| *p == path && *l == len && *m == modified)
        {
            return Ok(Some(summary.clone()));
        }
        let Some(summary) = summarize_session(claude_home, cwd, session_id)? else {
            return Ok(None);
        };
        let mut summaries = lock(&self.summaries);
        summaries.retain(|(p, ..)| *p != path);
        summaries.push_back((path, len, modified, summary.clone()));
        while summaries.len() > CACHED_SUMMARIES {
            summaries.pop_front();
        }
        Ok(Some(summary))
    }

    /// Lets go of what is kept of session `session_id`'s record.
    pub fn forget(&self, session_id: &str) {
        let named = |path: &Path| path.file_stem().is_some_and(|stem| stem == session_id);
        lock(&self.records).retain(|r| !named(&r.path));
        lock(&self.summaries).retain(|(p, ..)| !named(p));
    }

    /// How many times a record file has been read and projected, rather than
    /// taken from the cache.
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

/// About how much memory `events` holds, erring high: each event's own
/// size twice over (the vector holding them may have room for as many again),
/// and what its strings, lists and JSON objects hold on the heap.
fn approximate_bytes(events: &[RecordEvent]) -> usize {
    events
        .iter()
        .map(|e| {
            let strings = [&e.message_id, &e.ts, &e.backend_version]
                .into_iter()
                .flatten()
                .map(String::len)
                .sum::<usize>();
            // The event's own fields are inside it, not in a map of their own.
            let as_json = |event: &Event| match serde_json::to_value(event) {
                Ok(serde_json::Value::Object(fields)) => fields.values().map(heap_bytes).sum(),
                Ok(other) => heap_bytes(&other),
                Err(_) => 0,
            };
            let fields = match &e.event {
                // A user message's attachments are enums in a list, which
                // their JSON objects would count many times over: they count
                // as their strings and the list itself, the rest as JSON.
                Event::UserMessage(message) if !message.attachments.is_empty() => {
                    let attachments: usize = message
                        .attachments
                        .iter()
                        .map(|attachment| match attachment {
                            MessageAttachment::Image { media_type, data } => {
                                media_type.len() + data.len()
                            }
                            MessageAttachment::File { path }
                            | MessageAttachment::Folder { path } => path.len(),
                        })
                        .sum();
                    // Built field by field, so no image is copied to be measured.
                    let without = Event::UserMessage(UserMessage {
                        text: message.text.clone(),
                        origin: message.origin,
                        peer: message.peer.clone(),
                        attachments: Vec::new(),
                    });
                    as_json(&without)
                        + attachments
                        + message.attachments.capacity() * size_of::<MessageAttachment>()
                }
                event => as_json(event),
            };
            2 * size_of::<RecordEvent>() + strings + fields
        })
        .sum()
}

/// About how many bytes `value`'s parts hold on the heap. An event is
/// counted as its JSON, where a struct becomes an object: that counts a
/// struct, which holds nothing of its own on the heap, as a map, which does,
/// so the count errs high.
fn heap_bytes(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    // An object is a B-tree whose nodes have room for eleven entries and are
    // kept at least half full; even a small one takes a whole node.
    const ENTRY: usize = size_of::<String>() + size_of::<Value>();
    const NODE: usize = 11 * ENTRY + 64;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => 0,
        Value::String(s) => s.len(),
        // A list read in grows by doubling, from room for four, so it may
        // have room for nearly twice what it holds. `value` is a copy made
        // to the size it holds, so its own capacity does not say.
        Value::Array(items) if items.is_empty() => 0,
        Value::Array(items) => {
            items.len().next_power_of_two().max(4) * size_of::<Value>()
                + items.iter().map(heap_bytes).sum::<usize>()
        }
        Value::Object(map) => {
            map.len().div_ceil(5).max(1) * NODE
                + map
                    .iter()
                    .map(|(k, v)| k.len() + heap_bytes(v))
                    .sum::<usize>()
        }
    }
}

/// Locks a mutex, carrying on if a panicking thread poisoned it: what it
/// guards is a cache, consistent between single updates.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether `id` has the shape of a Claude Code session id: a UUID in its
/// hyphenated form. Ids read from file names, or handed to `--resume`, are
/// checked with this before use.
pub fn is_session_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    let lengths = [8, 4, 4, 4, 12];
    groups.len() == lengths.len()
        && groups
            .iter()
            .zip(lengths)
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Counts the bytes the current thread holds on the heap, so a test can tell
/// how much memory a projection really takes. Test-only: it wraps the system
/// allocator, which needs `unsafe`.
#[cfg(test)]
#[allow(unsafe_code)]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct Counting;

    thread_local! {
        static HELD: Cell<isize> = const { Cell::new(0) };
    }

    fn add(bytes: isize) {
        // `try_with`: a thread being torn down still frees memory.
        let _ = HELD.try_with(|held| held.set(held.get() + bytes));
    }

    /// Bytes allocated and not yet freed on this thread.
    pub fn held() -> isize {
        HELD.with(Cell::get)
    }

    // SAFETY: every call is passed straight to the system allocator.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            add(layout.size() as isize);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            add(-(layout.size() as isize));
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            add(new_size as isize - layout.size() as isize);
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The session-record fixtures, as text.
    fn record_fixtures() -> Vec<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/briefs/protocol/fixtures/claude-code");
        let mut found = Vec::new();
        for version in fs::read_dir(root).unwrap() {
            let Ok(files) = fs::read_dir(version.unwrap().path().join("record")) else {
                continue;
            };
            for file in files {
                found.push(fs::read_to_string(file.unwrap().path()).unwrap());
            }
        }
        assert!(!found.is_empty(), "no session-record fixtures found");
        found
    }

    #[test]
    fn the_cache_budget_counts_what_a_projection_holds() {
        // A large tool input: JSON objects take many times their text in
        // memory.
        let input: serde_json::Map<String, serde_json::Value> = (0..2000)
            .map(|i| {
                (
                    format!("k{i}"),
                    serde_json::json!({"a": [1, 2, true], "b": "x"}),
                )
            })
            .collect();
        let tool = serde_json::json!({"type": "assistant", "uuid": "a1", "parentUuid": null,
            "message": {"id": "m1", "role": "assistant", "content": [
                {"type": "tool_use", "id": "t1", "name": "Edit", "input": input}]}});
        // A list just past a power of two: read in, it has room for twice
        // as many items as it holds.
        let list = serde_json::json!({"type": "assistant", "uuid": "a2", "parentUuid": null,
            "message": {"id": "m2", "role": "assistant", "content": [
                {"type": "tool_use", "id": "t2", "name": "Edit",
                    "input": {"items": vec![0; 1025]}}]}});
        let mut records = record_fixtures();
        records.push(format!("{tool}\n"));
        records.push(format!("{list}\n"));
        for text in records {
            let before = counting::held();
            let events = projection::project_record(&text);
            let held = (counting::held() - before) as usize;
            let counted = approximate_bytes(&events);
            // Never under what the events hold, so the cache keeps to its
            // budget, and not far over, so it keeps what fits.
            assert!(counted >= held, "counted {counted} bytes of {held}");
            assert!(counted <= 2 * held, "counted {counted} bytes of {held}");
        }
    }

    /// A file in memory whose next write takes only `take` bytes, and which
    /// another writer may append to while that write happens.
    struct ShortFile {
        bytes: Vec<u8>,
        take: usize,
        meanwhile: &'static [u8],
    }

    impl AppendTarget for ShortFile {
        fn len(&mut self) -> io::Result<u64> {
            Ok(self.bytes.len() as u64)
        }
        fn last_byte(&mut self) -> io::Result<Option<u8>> {
            Ok(self.bytes.last().copied())
        }
        fn append(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let taken = self.take.min(bytes.len());
            self.bytes.extend_from_slice(&bytes[..taken]);
            self.bytes.extend_from_slice(self.meanwhile);
            // Only the first write is short.
            self.take = usize::MAX;
            self.meanwhile = b"";
            Ok(taken)
        }
    }

    const LINE: &[u8] = b"{\"type\":\"custom-title\"}\n";

    // verifies: PROTO-CLAUDE-CODE-STREAM-R22
    #[test]
    fn a_partial_write_is_ended_never_cut_and_reported() {
        let mut file = ShortFile {
            bytes: b"{}\n".to_vec(),
            take: 7,
            meanwhile: b"",
        };
        let error = append_line(&mut file, LINE).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        assert_eq!(file.bytes, b"{}\n{\"type\"\n");
        // A CLI line that landed after the torn part is kept whole.
        let mut file = ShortFile {
            bytes: b"{}\n".to_vec(),
            take: 7,
            meanwhile: b"{\"cli\":1}\n",
        };
        append_line(&mut file, LINE).unwrap_err();
        assert_eq!(file.bytes, b"{}\n{\"type\"{\"cli\":1}\n\n");
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R22
    #[test]
    fn a_record_whose_last_line_is_unterminated_is_not_written() {
        let mut file = ShortFile {
            bytes: b"{}\n{\"half".to_vec(),
            take: usize::MAX,
            meanwhile: b"",
        };
        let error = append_line(&mut file, LINE).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::ResourceBusy);
        assert_eq!(file.bytes, b"{}\n{\"half");
        // An empty record, or one ending its last line, takes the entry whole.
        for start in [&b""[..], b"{}\n"] {
            let mut file = ShortFile {
                bytes: start.to_vec(),
                take: usize::MAX,
                meanwhile: b"",
            };
            append_line(&mut file, LINE).unwrap();
            assert_eq!(file.bytes, [start, LINE].concat());
        }
    }

    #[test]
    fn encodes_like_the_cli() {
        assert_eq!(encoded_cwd(r"C:\Users\user\ersa"), "C--Users-user-ersa");
        assert_eq!(encoded_cwd("/home/jo/my repo"), "-home-jo-my-repo");
        assert_eq!(encoded_cwd("C:/é"), "C---");
        // One emoji is two UTF-16 code units, so two dashes.
        assert_eq!(encoded_cwd("a😀b"), "a--b");
    }

    #[test]
    fn cuts_long_names_and_adds_the_path_hash() {
        let long = format!("C:/{}", "a".repeat(300));
        let encoded = encoded_cwd(&long);
        let (cut, hash) = encoded.split_at(MAX_ENCODED_LEN);
        assert_eq!(cut, &format!("C--{}", "a".repeat(197)));
        assert_eq!(hash, format!("-{}", path_hash(&long)));
        assert!(encoded_cwd(&"a".repeat(200)).len() == 200);
    }

    #[test]
    fn hashes_like_javascript() {
        // Values computed with the same function in Node.js:
        // (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = (h << 5) - h + s.charCodeAt(i) | 0; return Math.abs(h).toString(36) }
        assert_eq!(path_hash(""), "0");
        assert_eq!(path_hash("a"), "2p");
        assert_eq!(path_hash("hello world"), "to5x38");
        assert_eq!(path_hash(&format!("C:/{}", "a".repeat(300))), "2el7y0");
    }

    #[test]
    fn session_ids_are_uuids() {
        assert!(is_session_id("d34f0c1b-64ee-437e-a4e3-3487df29a506"));
        for bad in [
            "",
            "--resume",
            "d34f0c1b-64ee-437e-a4e3-3487df29a50",
            "d34f0c1b-64ee-437e-a4e3-3487df29a5066",
            "d34f0c1b_64ee-437e-a4e3-3487df29a506",
            "g34f0c1b-64ee-437e-a4e3-3487df29a506",
            "../../etc/passwd",
        ] {
            assert!(!is_session_id(bad), "{bad}");
        }
    }
}
