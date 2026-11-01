//! The CLI's live-session registry, read before resuming
//! (PROTO-CLAUDE-CODE-STREAM-R17).
//!
//! Each running CLI process writes `<claude home>/sessions/<pid>.json` naming
//! the session it has open, and removes it when it exits. A process that
//! crashed leaves its file behind, so a file counts only while its process
//! is still running.

use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::platform;

/// A registry file is a few hundred bytes; anything far larger is not one,
/// and no more than this is read.
const MAX_REGISTRY_FILE: u64 = 64 * 1024;

/// What the registry says about one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveCheck {
    /// No running process has it open.
    NotOpen,
    /// A running process has it open. Resuming it too would interleave both
    /// transcripts in one record, so Ersa says so.
    Open(LiveSession),
    /// No readable file names it, but these could not be read (a file of a
    /// shape this version does not know, say, or something that is not a
    /// file), so one of them may. Ersa says it could not tell, rather than
    /// that the session is free.
    Unknown { unreadable: Vec<PathBuf> },
}

/// A running CLI process that has a session open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub pid: u32,
    /// The working directory the process runs in.
    pub cwd: Option<String>,
    /// What started it, such as `cli`, `claude-vscode` or `ersa`.
    pub entrypoint: Option<String>,
}

/// The fields of a registry file the adapter reads; the file has more, and
/// they are the CLI's own business. A file with no session id names no
/// session (a process between sessions); one with no pid is named by it.
#[derive(Deserialize)]
struct RegistryFile {
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default, rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    entrypoint: Option<String>,
}

/// Whether a running process has session `session_id` open. The check
/// informs the user and never stops a resume. With no registry directory at
/// all, no CLI process has run, so nothing is open; a directory that cannot
/// be read makes the answer unknown.
///
/// A pid the system has since given to an unrelated process makes a stale
/// file look live; the cost is a warning that was not needed.
pub fn live_session(claude_home: &Path, session_id: &str) -> LiveCheck {
    let dir = claude_home.join("sessions");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return LiveCheck::NotOpen,
        Err(_) => {
            return LiveCheck::Unknown {
                unreadable: vec![dir],
            };
        }
    };
    let mut unreadable = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            unreadable.push(dir.clone());
            continue;
        };
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let file = match read_registry_file(&path) {
            Read::File(file) => Some(file),
            // A CLI removed its file as it exited.
            Read::Gone => continue,
            // Another CLI may be writing it this moment: read it once more.
            Read::Unreadable => match read_registry_file(&path) {
                Read::File(file) => Some(file),
                Read::Gone => continue,
                Read::Unreadable => None,
            },
        };
        // The file is named by its pid.
        let named_pid = path
            .file_stem()
            .and_then(|s| s.to_str()?.parse::<u32>().ok());
        let Some(file) = file else {
            // When that process has gone, the file is a crashed CLI's
            // leftover and says nothing about now.
            if named_pid.is_none_or(platform::is_running) {
                unreadable.push(path);
            }
            continue;
        };
        // Session ids are UUIDs, which may be written in either case.
        let names_it = file
            .session_id
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case(session_id));
        if !names_it {
            continue;
        }
        let Some(pid) = file.pid.or(named_pid) else {
            // It names the session but no process to check.
            unreadable.push(path);
            continue;
        };
        if platform::is_running(pid) {
            return LiveCheck::Open(LiveSession {
                pid,
                cwd: file.cwd,
                entrypoint: file.entrypoint,
            });
        }
    }
    if unreadable.is_empty() {
        LiveCheck::NotOpen
    } else {
        unreadable.sort();
        LiveCheck::Unknown { unreadable }
    }
}

/// The outcome of reading one registry file.
enum Read {
    File(RegistryFile),
    /// It no longer exists.
    Gone,
    /// It is not a regular file of the expected shape and size.
    Unreadable,
}

/// One registry file, read through a bound so a file that keeps growing
/// cannot make the read unbounded.
fn read_registry_file(path: &Path) -> Read {
    let gone_or_unreadable = |e: io::Error| match e.kind() {
        io::ErrorKind::NotFound => Read::Gone,
        _ => Read::Unreadable,
    };
    // `symlink_metadata` so a link is not followed, and a FIFO is never opened.
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Read::Unreadable,
        Err(e) => return gone_or_unreadable(e),
    }
    let mut text = String::new();
    let read = File::open(path)
        .and_then(|file| file.take(MAX_REGISTRY_FILE + 1).read_to_string(&mut text));
    if let Err(e) = read {
        return gone_or_unreadable(e);
    }
    if text.len() as u64 > MAX_REGISTRY_FILE {
        return Read::Unreadable;
    }
    match serde_json::from_str(&text) {
        Ok(file) => Read::File(file),
        Err(_) => Read::Unreadable,
    }
}
