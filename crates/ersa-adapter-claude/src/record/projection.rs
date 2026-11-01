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

/// One event projected from a record entry: `message_id` is the entry's
/// `uuid`, `ts` its `timestamp` and `backend_version` its `version`.
pub use ersa_adapter::RecordEvent;

/// The events of a whole session record, as read from its file.
///
/// The CLI ends every entry with a newline and appends as it runs, so the
/// last line of a live session's record may be half written. A last line
/// with no newline counts only when it is a whole JSON entry; otherwise it is
/// left, silently, for the next read. Any other line that cannot be read
/// yields an error and a passthrough of its start (PROTO-AGENT-EVENTS-R10),
/// and the pass carries on. Pure: the same record always yields the same
/// events.
///
/// Read in two passes over the text, one entry at a time: the first keeps
/// only what the conversation's chain needs of each entry, the second reads
/// each entry on the chain again to project it. A record of many megabytes
/// is never held as JSON all at once.
pub fn project_record(record: &str) -> Vec<RecordEvent> {
    let (complete, unterminated) = match record.rfind('\n') {
        Some(end) => (&record[..end], &record[end + 1..]),
        None => ("", record),
    };
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
            Ok(value) => Line::Entry {
                text: line,
                chain: chain_fields(&value),
            },
            Err(e) => Line::Failed(unreadable(line, format!("a record entry is not JSON: {e}"))),
        });
    }
    if unterminated.len() <= MAX_ENTRY_BYTES
        && let Ok(value) = serde_json::from_str::<Value>(unterminated)
    {
        lines.push(Line::Entry {
            text: unterminated,
            chain: chain_fields(&value),
        });
    }

    let entries: Vec<&Value> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Entry { chain, .. } => Some(chain),
            Line::Failed(_) => None,
        })
        .collect();
    let Conversation { chain, problem } = conversation(&entries);

    let mut projector = RecordProjector::default();
    let mut out = Vec::new();
    if let Some(problem) = problem {
        out.push(RecordEvent {
            message_id: None,
            ts: None,
            backend_version: None,
            event: protocol_error(problem),
        });
    }
    let mut projected_ids = HashSet::new();
    for line in lines {
        match line {
            Line::Failed(events) => out.extend(events),
            Line::Entry {
                text,
                chain: fields,
            } => {
                // An entry on another branch, or a subagent's, is not part of
                // the conversation (PROTO-CLAUDE-CODE-STREAM-R20). Entries
                // without an id (titles) are let through; they yield nothing.
                let off_branch = match (&chain, uuid(&fields)) {
                    (Some(chain), Some(id)) => !chain.contains(id),
                    // In file order, still only the main thread's entries.
                    (None, _) => !is_main_thread(&fields),
                    (Some(_), None) => false,
                };
                // An entry written twice (a resume can repeat one) counts once.
                let repeated =
                    uuid(&fields).is_some_and(|id| !projected_ids.insert(id.to_string()));
                // Read whole again only now, one entry at a time. It parsed in
                // the first pass, so it parses here.
                if !off_branch
                    && !repeated
                    && let Ok(value) = serde_json::from_str::<Value>(text)
                {
                    out.extend(projector.project(value));
                }
            }
        }
    }
    out
}

/// Whether an entry belongs to the main conversation rather than to a
/// subagent (`isSidechain`), a teammate (`teamName`) or a progress report.
fn is_main_thread(value: &Value) -> bool {
    value.get("isSidechain").and_then(Value::as_bool) != Some(true)
        && value.get("teamName").is_none()
        && value.get("type").and_then(Value::as_str) != Some("progress")
}

enum Line<'a> {
    Entry {
        /// The entry's text, read again to project it.
        text: &'a str,
        /// Only the fields the conversation's chain reads.
        chain: Value,
    },
    /// The events already reported for a line that could not be read.
    Failed(Vec<RecordEvent>),
}

/// The fields of an entry the conversation's chain and the main-thread test
/// read; everything else (the message, often the bulk of the line) is left
/// in the text until the entry is projected.
fn chain_fields(value: &Value) -> Value {
    let mut kept = serde_json::Map::new();
    for key in [
        "uuid",
        "parentUuid",
        "logicalParentUuid",
        "isSidechain",
        "teamName",
        "type",
    ] {
        if let Some(field) = value.get(key) {
            kept.insert(key.to_string(), field.clone());
        }
    }
    Value::Object(kept)
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

/// Which entries make the conversation.
struct Conversation {
    /// The ids of the entries on its chain, or `None` when every
    /// main-thread entry counts, in file order.
    chain: Option<HashSet<String>>,
    /// Why the chain could not be followed whole, for an error event.
    problem: Option<String>,
}

/// The conversation's chain. It ends at the latest main-thread leaf (an entry
/// no main-thread entry names as its parent) that leads back to a message,
/// and runs back through `parentUuid`. Where `parentUuid` is null after a
/// compaction, `logicalParentUuid` continues it, so the history before the
/// compaction stays.
///
/// A damaged record never silently loses history. When the chain names a
/// parent the record does not have (a line that could not be read, say),
/// every main-thread entry before the break counts too, in file order; when
/// no chain reaches a message at all, every main-thread entry counts. Either
/// way the problem is reported.
fn conversation(entries: &[&Value]) -> Conversation {
    let every_entry = |problem| Conversation {
        chain: None,
        problem,
    };
    // A record that links no entries at all (an older or hand-made one).
    if !entries.iter().any(|e| e.get("parentUuid").is_some()) {
        return every_entry(None);
    }
    let mut nodes: HashMap<&str, Node> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(id) = uuid(entry) else {
            continue;
        };
        let text = |key: &str| entry.get(key).and_then(Value::as_str);
        let kind = text("type");
        // An entry written twice keeps its first place, where it is
        // projected.
        nodes.entry(id).or_insert(Node {
            index,
            parent: text("parentUuid").or_else(|| text("logicalParentUuid")),
            main: is_main_thread(entry),
            message: matches!(kind, Some("user" | "assistant")),
        });
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
    // skips trailing entries that are not messages. An entry walked from an
    // earlier leaf without reaching a message leads to none from any leaf,
    // so each entry is walked at most once (and a cycle ends the walk).
    let mut walked = HashSet::new();
    let mut last_message = None;
    'leaves: for (leaf, _) in leaves {
        let mut at = Some(leaf);
        while let Some(id) = at {
            let Some(node) = nodes.get(id) else {
                break;
            };
            if !walked.insert(id) {
                break;
            }
            if node.message {
                last_message = Some(id);
                break 'leaves;
            }
            at = node.parent;
        }
    }
    let Some(last_message) = last_message else {
        let has_messages = nodes.values().any(|n| n.main && n.message);
        return every_entry(has_messages.then(|| {
            "the record's entries form no chain to a message; its entries are shown in file order"
                .to_string()
        }));
    };

    let mut chain = HashSet::new();
    let mut at = last_message;
    loop {
        if !chain.insert(at.to_string()) {
            break; // A cycle: the chain is complete as far as it goes.
        }
        let node = &nodes[at];
        match node.parent {
            None => break,
            Some(parent) if nodes.contains_key(parent) => at = parent,
            Some(parent) => {
                // The chain names an entry the record does not have.
                let root = node.index;
                for (id, earlier) in &nodes {
                    if earlier.main && earlier.index < root {
                        chain.insert(id.to_string());
                    }
                }
                return Conversation {
                    chain: Some(chain),
                    problem: Some(format!(
                        "the record's chain names entry {parent}, which it does not have; \
                         the entries before the break are shown in file order"
                    )),
                };
            }
        }
    }
    Conversation {
        chain: Some(chain),
        problem: None,
    }
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
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(String::from);
        let ts = text("timestamp");
        let version = text("version");
        let attribute = |event| RecordEvent {
            message_id: id.clone(),
            ts: ts.clone(),
            backend_version: version.clone(),
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
        backend_version: None,
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
