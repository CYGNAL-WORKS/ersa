//! The projection of a session record to Ersa events.
//!
//! A `user` or `assistant` entry's `message` is the message the live stream
//! carried with the same `uuid`, so it goes through the same functions as the
//! stream does and yields the same events (PROTO-CLAUDE-CODE-STREAM-R15). What
//! only the live stream carries (text as it arrives, permission prompts, turn
//! ends, cost, rate limits) is not in the record, so neither are the events it
//! yields: the record has no turns, only messages.
//!
//! The record is not a flat transcript. Each entry names its predecessor in
//! `parentUuid`, and rewinding or editing a prompt in the terminal starts a new
//! branch from an earlier entry while the old branch stays in the file. Entries
//! of subagents (`isSidechain`) can sit in it too. The conversation is the
//! chain that ends at the latest main-thread entry, followed back through
//! `parentUuid`, as the Agent SDK's reader does; only its messages are
//! projected.

use std::collections::{HashMap, HashSet};

use ersa_protocol::events::{
    BackendPassthrough, ErrorCode, ErrorEvent, Event, MessageOrigin, UserMessage,
};
use serde::Deserialize;
use serde_json::Value;

use crate::BACKEND;
use crate::projector::{kind_of, plain_text, reply_events, tool_result_events};
use crate::wire::{MessageContent, RecordEntry, RecordUser};

/// The longest record line read as an entry. A tool result can run to
/// megabytes (an image, a large file); anything far beyond is not an entry
/// the transcript can use, and parsing it would cost more than it is worth.
const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;

/// How much of a line that could not be read a passthrough event keeps.
const PASSTHROUGH_PREFIX_BYTES: usize = 4 * 1024;

/// One event projected from a record entry.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordEvent {
    /// The entry's `uuid`: the backend's message id, on which record and live
    /// input are merged. `None` for a line that could not be read.
    pub message_id: Option<String>,
    /// When the CLI wrote the entry, as the entry gives it.
    pub ts: Option<String>,
    pub event: Event,
}

/// The events of a whole session record, as read from its file.
///
/// Only lines a newline ends are entries: the CLI appends as it runs, so the
/// last line of a live session's record may be half written, and is left for
/// the next read. A complete line that cannot be read yields an error and a
/// passthrough of its start (PROTO-AGENT-EVENTS-R10), and the pass carries on.
/// Pure: the same record always yields the same events.
pub fn project_record(record: &str) -> Vec<RecordEvent> {
    let complete = record.rfind('\n').map_or("", |end| &record[..end]);
    let mut lines = Vec::new();
    for line in complete.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_ENTRY_BYTES {
            lines.push(Line::Failed(unreadable(
                line,
                format!("a record entry is longer than {MAX_ENTRY_BYTES} bytes"),
            )));
            continue;
        }
        lines.push(match serde_json::from_str::<Value>(line) {
            Ok(value) => Line::Entry(value),
            Err(e) => Line::Failed(unreadable(line, format!("a record entry is not JSON: {e}"))),
        });
    }

    let entries: Vec<&Value> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Entry(value) => Some(value),
            Line::Failed(_) => None,
        })
        .collect();
    let conversation = conversation(&entries);

    let mut projector = RecordProjector::default();
    let mut out = Vec::new();
    for line in lines {
        match line {
            Line::Failed(events) => out.extend(events),
            Line::Entry(value) => {
                // An entry on another branch, or a subagent's, is not part of
                // the conversation. Entries without an id (titles) are let
                // through; they yield nothing.
                let off_branch = match (&conversation, uuid(&value)) {
                    (Some(chain), Some(id)) => !chain.contains(id),
                    _ => false,
                };
                if !off_branch {
                    out.extend(projector.project(value));
                }
            }
        }
    }
    out
}

enum Line {
    Entry(Value),
    /// The events already reported for a line that could not be read.
    Failed(Vec<RecordEvent>),
}

fn uuid(value: &Value) -> Option<&str> {
    value.get("uuid").and_then(Value::as_str)
}

/// One entry, as the chain sees it.
struct Node<'a> {
    index: usize,
    parent: Option<&'a str>,
    main: bool,
    message: bool,
}

/// The ids of the entries on the conversation's chain, or `None` when the
/// record links no entries at all (then every entry counts, in file order).
///
/// The chain ends at the latest main-thread leaf (an entry no main-thread
/// entry names as its parent) that leads back to a message, and runs back
/// through `parentUuid`. Where `parentUuid` is null after a compaction,
/// `logicalParentUuid` continues it, so the history before the compaction
/// stays.
fn conversation(entries: &[&Value]) -> Option<HashSet<String>> {
    if !entries.iter().any(|e| e.get("parentUuid").is_some()) {
        return None;
    }
    let mut nodes: HashMap<&str, Node> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(id) = uuid(entry) else {
            continue;
        };
        let text = |key: &str| entry.get(key).and_then(Value::as_str);
        let kind = text("type");
        nodes.insert(
            id,
            Node {
                index,
                parent: text("parentUuid").or_else(|| text("logicalParentUuid")),
                main: entry.get("isSidechain").and_then(Value::as_bool) != Some(true)
                    && entry.get("teamName").is_none()
                    && kind != Some("progress"),
                message: matches!(kind, Some("user" | "assistant")),
            },
        );
    }

    let parents: HashSet<&str> = nodes
        .values()
        .filter(|n| n.main)
        .filter_map(|n| n.parent)
        .collect();
    let mut leaves: Vec<(&str, &Node)> = nodes
        .iter()
        .filter(|(id, n)| n.main && !parents.contains(*id))
        .map(|(id, n)| (*id, n))
        .collect();
    leaves.sort_by_key(|(_, n)| std::cmp::Reverse(n.index));

    // The latest leaf whose chain reaches a message; walking back from it
    // skips trailing entries that are not messages.
    let mut last_message = None;
    'leaves: for (leaf, _) in leaves {
        let mut seen = HashSet::new();
        let mut at = Some(leaf);
        while let Some(id) = at {
            let Some(node) = nodes.get(id) else {
                break;
            };
            if !seen.insert(id) {
                break; // A cycle: a damaged record.
            }
            if node.message {
                last_message = Some(id);
                break 'leaves;
            }
            at = node.parent;
        }
    }

    let mut chain = HashSet::new();
    let mut at = last_message;
    while let Some(id) = at {
        if !chain.insert(id.to_string()) {
            break;
        }
        at = nodes.get(id).and_then(|n| n.parent);
    }
    Some(chain)
}

/// Projects record entries one by one, in order. Like the stream's projector
/// it numbers each message's content blocks across the entries that carry
/// them.
#[derive(Debug, Default)]
struct RecordProjector {
    blocks_seen: HashMap<String, u64>,
}

impl RecordProjector {
    /// The events of one entry. Entries that carry no message (titles,
    /// snapshots, attachments, and whatever else the CLI keeps) yield none.
    fn project(&mut self, value: Value) -> Vec<RecordEvent> {
        let id = uuid(&value).map(String::from);
        let ts = value
            .get("timestamp")
            .and_then(Value::as_str)
            .map(String::from);
        let attribute = |event| RecordEvent {
            message_id: id.clone(),
            ts: ts.clone(),
            event,
        };

        // Typed from a reference rather than a clone of the whole entry.
        // `RecordEntry` keeps any entry it cannot type, so the fallback (which
        // the arms below never look inside) only satisfies the type checker.
        let entry = RecordEntry::deserialize(&value).unwrap_or(RecordEntry::Other(Value::Null));
        let events = match entry {
            RecordEntry::Assistant(reply) => reply_events(
                &mut self.blocks_seen,
                reply.message,
                reply.error.as_deref(),
                // Subagents' entries are not on the conversation's chain.
                None,
            ),
            RecordEntry::User(user) => user_events(user),
            RecordEntry::Other(_) => {
                let kind = value.get("type").and_then(Value::as_str);
                if matches!(kind, Some("user" | "assistant")) {
                    // A message entry whose shape has drifted: say so, and
                    // keep it (PROTO-AGENT-EVENTS-R10).
                    let kind = kind.unwrap_or_default();
                    return vec![
                        attribute(protocol_error(format!(
                            "a {kind} record entry did not have the expected shape"
                        ))),
                        attribute(passthrough(value)),
                    ];
                }
                return Vec::new();
            }
            // Titles serve the session list, not the transcript.
            RecordEntry::CustomTitle(_) | RecordEntry::AiTitle(_) | RecordEntry::LastPrompt(_) => {
                return Vec::new();
            }
        };
        if events.is_empty() {
            // A message entry that maps to nothing else is kept as it came
            // (PROTO-AGENT-EVENTS-R02).
            return vec![attribute(passthrough(value))];
        }
        events.into_iter().map(attribute).collect()
    }
}

/// A `user` entry: tool results, as on the stream, or a prompt. Prompts are not
/// on the stream (Ersa sends them, and does not ask for them back), so the
/// record is where they come from.
fn user_events(user: RecordUser) -> Vec<Event> {
    let from_cli = user.is_from_cli();
    let content = user.message.content;
    let results = tool_result_events(content.clone());
    if !results.is_empty() {
        return results;
    }
    let text = match content {
        MessageContent::Text(text) => Some(text),
        MessageContent::Blocks(blocks) => plain_text(&blocks),
    };
    match text {
        Some(text) => vec![Event::UserMessage(UserMessage {
            text,
            origin: if from_cli {
                MessageOrigin::System
            } else {
                MessageOrigin::Human
            },
            peer: None,
        })],
        None => Vec::new(),
    }
}

/// The events of a line that could not be read: an error, and the line's
/// start, so a huge line is not copied whole into an event.
fn unreadable(line: &str, message: String) -> Vec<RecordEvent> {
    let mut end = line.len().min(PASSTHROUGH_PREFIX_BYTES);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    let event = |event| RecordEvent {
        message_id: None,
        ts: None,
        event,
    };
    vec![
        event(protocol_error(message)),
        event(Event::BackendPassthrough(BackendPassthrough {
            backend: BACKEND.into(),
            kind: "unparsed".into(),
            payload: Value::String(line[..end].to_string()),
        })),
    ]
}

fn passthrough(payload: Value) -> Event {
    Event::BackendPassthrough(BackendPassthrough {
        backend: BACKEND.into(),
        kind: kind_of(&payload),
        payload,
    })
}

fn protocol_error(message: String) -> Event {
    Event::Error(ErrorEvent {
        code: ErrorCode::Protocol,
        message,
        recoverable: true,
        action: None,
    })
}
