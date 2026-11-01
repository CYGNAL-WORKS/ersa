//! The CLI's live-session registry, read before resuming
//! (PROTO-CLAUDE-CODE-STREAM-R17).
//!
//! Each running CLI process writes `<claude home>/sessions/<pid>.json` naming
//! the session it has open, and removes it when it exits. A process that
//! crashed leaves its file behind, so a file counts only while its process
//! is still running.

use std::fs::{self, File};
use std::io::Read;
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
    /// No readable file names it, but these files could not be read, so one
    /// of them may: the CLI may have changed the registry's format. Ersa says
    /// it could not tell, rather than that the session is free.
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
/// they are the CLI's own business. A file without a session id or a pid is
/// not one this version understands.
#[derive(Deserialize)]
struct RegistryFile {
    pid: u32,
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    entrypoint: Option<String>,
}

/// Whether a running process has session `session_id` open. The check
/// informs the user and never stops a resume. With no registry directory at
/// all, no CLI process has run, so nothing is open.
///
/// A pid the system has since given to an unrelated process makes a stale
/// file look live; the cost is a warning that was not needed.
pub fn live_session(claude_home: &Path, session_id: &str) -> LiveCheck {
    let Ok(entries) = fs::read_dir(claude_home.join("sessions")) else {
        return LiveCheck::NotOpen;
    };
    let mut unreadable = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(file) = read_registry_file(&path) else {
            unreadable.push(path);
            continue;
        };
        if file.session_id != session_id {
            continue;
        }
        if platform::is_running(file.pid) {
            return LiveCheck::Open(LiveSession {
                pid: file.pid,
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

/// One registry file, read through a bound so a file that keeps growing
/// cannot make the read unbounded. `None` when it is not a regular file of
/// the expected shape.
fn read_registry_file(path: &Path) -> Option<RegistryFile> {
    // `symlink_metadata` so a link is not followed, and a FIFO is never opened.
    if !fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(MAX_REGISTRY_FILE + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MAX_REGISTRY_FILE {
        return None;
    }
    serde_json::from_str(&text).ok()
}
