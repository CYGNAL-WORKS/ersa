//! The CLI's live-session registry, read before resuming
//! (PROTO-CLAUDE-CODE-STREAM-R17).
//!
//! Each running CLI process writes `<claude home>/sessions/<pid>.json` naming
//! the session it has open, and removes it when it exits. A process that
//! crashed leaves its file behind, so a file counts only while its process
//! is still running.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::alive;

/// A registry file is a few hundred bytes; anything far larger is not one,
/// and is not read.
const MAX_REGISTRY_FILE: u64 = 64 * 1024;

/// A running CLI process that has a session open. Resuming that session too
/// would interleave both transcripts in one record, so Ersa says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub pid: u32,
    /// The working directory the process runs in.
    pub cwd: Option<String>,
    /// What started it, such as `cli`, `claude-vscode` or `ersa`.
    pub entrypoint: Option<String>,
}

/// The fields of a registry file the adapter reads; the file has more, and
/// they are the CLI's own business.
#[derive(Deserialize)]
struct RegistryFile {
    pid: Option<u32>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    entrypoint: Option<String>,
}

/// The running process, if any, that has session `session_id` open. A
/// missing registry, or a file that cannot be read, reports nothing: the
/// check informs the user and never stops a resume.
///
/// A pid the system has since given to an unrelated process makes a stale
/// file look live; the cost is a warning that was not needed.
pub fn live_session(claude_home: &Path, session_id: &str) -> Option<LiveSession> {
    let entries = fs::read_dir(claude_home.join("sessions")).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_REGISTRY_FILE {
            continue;
        }
        let Some(file) = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<RegistryFile>(&text).ok())
        else {
            continue;
        };
        if file.session_id.as_deref() != Some(session_id) {
            continue;
        }
        // The file is named by its pid; the field says the same.
        let pid = file
            .pid
            .or_else(|| path.file_stem()?.to_str()?.parse().ok());
        let Some(pid) = pid else {
            continue;
        };
        if alive::is_running(pid) {
            return Some(LiveSession {
                pid,
                cwd: file.cwd,
                entrypoint: file.entrypoint,
            });
        }
    }
    None
}
