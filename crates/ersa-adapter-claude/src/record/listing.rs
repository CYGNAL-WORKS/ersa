//! Listing a directory's sessions from their records
//! (PROTO-CLAUDE-CODE-STREAM-R16).
//!
//! A record grows with every turn and can run to many megabytes, and a list
//! may cover hundreds of them, so only the head and the tail of each file are
//! read: the first prompt is near the start, and titles and the latest prompt
//! are appended, so the newest are near the end.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ersa_adapter::RecordedSettings;

use super::settings::settings;
use super::{is_session_id, project_dir};
use crate::wire::{ContentBlock, MessageContent, RecordEntry, RecordUser};

/// How much of each end of a record is read.
pub(super) const CHUNK: u64 = 64 * 1024;

/// The longest title, in characters, its ellipsis included; a prompt can be
/// pages long. It is also the longest a session can be renamed to, which the
/// adapter states as its title rules.
pub(crate) const MAX_TITLE_CHARS: usize = 200;

/// A directory's sessions, and what could not be read, so a caller can say
/// that some sessions are missing rather than show a shorter list as whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionList {
    /// Most recently changed first.
    pub sessions: Vec<SessionSummary>,
    /// Records that could not be read (locked, access denied, vanished while
    /// being read). The directory itself is listed when an entry of it could
    /// not be read.
    pub unreadable: Vec<PathBuf>,
}

/// One session in a directory's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    /// The CLI's session id, which `--resume` takes.
    pub session_id: String,
    /// The latest `custom-title`, else the latest `ai-title`, else the last
    /// prompt, else the first prompt; `None` when the record has none of them.
    pub title: Option<String>,
    /// When the record last changed.
    pub modified: Option<SystemTime>,
    /// When the session began: the `timestamp` of the record's first entry
    /// that has one, as the record gives it.
    pub created_at: Option<String>,
    /// The model, effort and permission mode the tail holds; a value only
    /// the rest of the record holds is none (PROTO-CLAUDE-CODE-STREAM-R59).
    pub settings: RecordedSettings,
}

/// The sessions the CLI recorded for working directory `cwd`. A directory
/// with no records lists nothing; a directory that cannot be read is an error;
/// a record that cannot be read is named in [`SessionList::unreadable`].
pub fn list_sessions(claude_home: &Path, cwd: &str) -> io::Result<SessionList> {
    let dir = project_dir(claude_home, cwd);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SessionList::default()),
        Err(e) => return Err(e),
    };
    let mut list = SessionList::default();
    for entry in entries {
        let Ok(entry) = entry else {
            list.unreadable.push(dir.clone());
            continue;
        };
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".jsonl")) else {
            continue;
        };
        // The directory also holds other files and per-session folders; a
        // record is named by its session id.
        if !is_session_id(id) {
            continue;
        }
        let path = entry.path();
        let metadata = match entry.metadata() {
            // A folder or link named like a record is not one.
            Ok(metadata) if !metadata.is_file() => continue,
            Ok(metadata) => metadata,
            Err(_) => {
                list.unreadable.push(path);
                continue;
            }
        };
        match read_ends(&path) {
            Ok(ends) => list.sessions.push(SessionSummary {
                session_id: id.to_string(),
                title: ends.title(),
                modified: metadata.modified().ok(),
                settings: settings(&ends.tail),
                created_at: ends.created_at,
            }),
            // The CLI's retention deleted it since the directory was read.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => list.unreadable.push(path),
        }
    }
    list.sessions.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    list.unreadable.sort();
    list.unreadable.dedup();
    Ok(list)
}

/// The summary of one session, read from the head and tail of its record
/// like a list's ([`super::find_record`] finds the record). `None` when the
/// session has no record.
pub fn summarize_session(
    claude_home: &Path,
    cwd: &str,
    session_id: &str,
) -> io::Result<Option<SessionSummary>> {
    let Some(path) = super::find_record(claude_home, cwd, session_id)? else {
        return Ok(None);
    };
    let modified = fs::metadata(&path)?.modified().ok();
    let ends = match read_ends(&path) {
        Ok(ends) => ends,
        // The CLI's retention deleted it since it was found.
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some(SessionSummary {
        session_id: session_id.to_string(),
        title: ends.title(),
        modified,
        settings: settings(&ends.tail),
        created_at: ends.created_at,
    }))
}

/// The complete entries in the first and last `CHUNK` bytes of a record. A
/// line cut by either boundary is dropped; a line the CLI is still writing
/// fails to parse and is dropped too. When the file is shorter than two
/// chunks the two overlap, which only repeats entries.
struct Ends {
    head: Vec<RecordEntry>,
    tail: Vec<RecordEntry>,
    /// The first `timestamp` in the head.
    created_at: Option<String>,
}

fn read_ends(path: &Path) -> io::Result<Ends> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();

    let mut head = Vec::new();
    (&mut file).take(CHUNK).read_to_end(&mut head)?;
    if head.len() as u64 >= len {
        // The whole file fits in one chunk.
        let entries = entries(&head, false, false);
        return Ok(Ends {
            head: entries.clone(),
            tail: entries,
            created_at: first_timestamp(&head),
        });
    }

    // Start one byte early, so the first line can be told apart from a cut
    // one: it is complete only when that byte ends the line before it.
    let start = len.saturating_sub(CHUNK + 1);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    file.take(CHUNK + 1).read_to_end(&mut tail)?;
    Ok(Ends {
        head: entries(&head, false, true),
        tail: entries(&tail, start > 0, false),
        created_at: first_timestamp(&head),
    })
}

/// The `timestamp` of the first complete line in `head` that has one. Read
/// from the raw JSON: the first entries are snapshots and other kinds the
/// typed entries do not keep.
fn first_timestamp(head: &[u8]) -> Option<String> {
    head.split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .find_map(|entry| entry.get("timestamp")?.as_str().map(String::from))
}

/// The entries of the complete lines in `bytes`, skipping the first segment
/// (cut, or the byte before a line) and the last (possibly cut) as asked.
pub(super) fn entries(bytes: &[u8], skip_first: bool, skip_last: bool) -> Vec<RecordEntry> {
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if skip_last {
        lines.pop();
    }
    let lines = if skip_first {
        &lines[1.min(lines.len())..]
    } else {
        &lines[..]
    };
    lines
        .iter()
        // A chunk boundary can split a character; only cut lines hold one.
        .filter_map(|line| std::str::from_utf8(line).ok())
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| RecordEntry::from_line(line).ok())
        .collect()
}

impl Ends {
    fn title(&self) -> Option<String> {
        // Latest first: the tail from its end, then the head from its end.
        let newest_first = || self.tail.iter().rev().chain(self.head.iter().rev());
        let custom = newest_first().find_map(|entry| match entry {
            RecordEntry::CustomTitle(t) => non_empty(&t.custom_title),
            _ => None,
        });
        let ai = || {
            newest_first().find_map(|entry| match entry {
                RecordEntry::AiTitle(t) => non_empty(&t.ai_title),
                _ => None,
            })
        };
        // The CLI appends a `last-prompt` entry after each prompt; the prompt
        // itself serves where that entry is missing.
        let last_prompt = || {
            self.tail.iter().rev().find_map(|entry| match entry {
                RecordEntry::LastPrompt(p) => p.last_prompt.as_deref().and_then(non_empty),
                RecordEntry::User(user) => prompt(user),
                _ => None,
            })
        };
        let first_prompt = || {
            self.head.iter().find_map(|entry| match entry {
                RecordEntry::User(user) => prompt(user),
                _ => None,
            })
        };
        custom
            .or_else(ai)
            .or_else(last_prompt)
            .or_else(first_prompt)
            .map(|title| one_line(&title))
    }
}

/// The text of a prompt someone typed: not a tool result, and not text the
/// CLI added itself.
fn prompt(user: &RecordUser) -> Option<String> {
    if user.is_from_cli() {
        return None;
    }
    match &user.message.content {
        MessageContent::Text(text) => non_empty(text),
        MessageContent::Blocks(blocks) => {
            if blocks
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolResult { .. }))
            {
                return None;
            }
            // Its images and list of attachments are not its words: a prompt
            // of attachments alone titles nothing (PROTO-CLAUDE-CODE-STREAM-R27).
            crate::attachments::split_blocks(blocks)
                .0
                .as_deref()
                .and_then(non_empty)
        }
    }
}

/// A title fit for one line of a list: whitespace runs, line breaks included,
/// become one space, and a long one is cut with an ellipsis.
fn one_line(title: &str) -> String {
    let flat = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_TITLE_CHARS {
        return flat;
    }
    let mut cut: String = flat.chars().take(MAX_TITLE_CHARS - 1).collect();
    cut.push('…');
    cut
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}
