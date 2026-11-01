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

use super::{is_session_id, project_dir};
use crate::projector::plain_text;
use crate::wire::{ContentBlock, MessageContent, RecordEntry, RecordUser};

/// How much of each end of a record is read.
const CHUNK: u64 = 64 * 1024;

/// The longest title, in characters, its ellipsis included; a prompt can be
/// pages long.
const MAX_TITLE_CHARS: usize = 200;

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

/// The complete entries in the first and last `CHUNK` bytes of a record. A
/// line cut by either boundary is dropped; a line the CLI is still writing
/// fails to parse and is dropped too. When the file is shorter than two
/// chunks the two overlap, which only repeats entries.
struct Ends {
    head: Vec<RecordEntry>,
    tail: Vec<RecordEntry>,
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
    })
}

/// The entries of the complete lines in `bytes`, skipping the first segment
/// (cut, or the byte before a line) and the last (possibly cut) as asked.
fn entries(bytes: &[u8], skip_first: bool, skip_last: bool) -> Vec<RecordEntry> {
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
            plain_text(blocks).as_deref().and_then(non_empty)
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
