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
use std::io::{self, BufRead, Read, Seek, SeekFrom};

use ersa_adapter::ConversationEnd;
use ersa_protocol::events::{
    AssistantText, BackendPassthrough, ErrorCode, ErrorEvent, Event, MessageOrigin, UserMessage,
};
use serde::Deserialize;
use serde_json::Value;

use crate::BACKEND;
use crate::projector::{kind_of, reply_events, tool_result_events};
use crate::wire::{LocalCommandOutput, MessageContent, RecordEntry, RecordUser};

/// The longest record line read as an entry. A tool result can run to
/// megabytes (an image, a large file); anything far beyond is not an entry
/// the transcript can use, and parsing it would cost more than it is worth.
const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;

/// How much of a line that could not be read a passthrough event keeps.
const PASSTHROUGH_PREFIX_BYTES: usize = 4 * 1024;

/// One event projected from a record entry: `message_id` is the entry's
/// `uuid`, `ts` its `timestamp` and `backend_version` its `version`.
pub use ersa_adapter::RecordEvent;

/// The events of a whole session record, as read from its text.
///
/// The CLI ends every entry with a newline and appends as it runs, so the
/// last line of a live session's record may be half written. A last line
/// with no newline counts only when it is a whole JSON entry; otherwise it is
/// left, silently, for the next read. Any other line that cannot be read
/// yields an error and a passthrough of its start (PROTO-AGENT-EVENTS-R10),
/// and the pass carries on. Pure: the same record always yields the same
/// events.
pub fn project_record(record: &str) -> Vec<RecordEvent> {
    let mut text = io::Cursor::new(record.as_bytes());
    project_record_from(&mut text, record.len() as u64).expect("text in memory always reads")
}

/// [`project_record`] over the first `limit` bytes `reader` gives, read
/// line by line in two passes: the first keeps only what the conversation's
/// chain needs of each entry and where the entry lies, the second reads each
/// entry on the chain again to project it. Neither the record's text nor its
/// entries as JSON are ever held whole. Bytes that are not UTF-8 are read
/// with replacement characters.
pub fn project_record_from<R: BufRead + Seek>(
    reader: &mut R,
    limit: u64,
) -> io::Result<Vec<RecordEvent>> {
    let lines = scan_lines(reader, limit)?.lines;
    let Conversation { chain, problem } = conversation_of(&lines);

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
                offset,
                len,
                chain: fields,
            } => {
                // An entry written twice (a resume can repeat one) counts once.
                let repeated =
                    uuid(&fields).is_some_and(|id| !projected_ids.insert(id.to_string()));
                if !on_conversation(&chain, &fields) || repeated {
                    continue;
                }
                if let Some(value) = entry_at(reader, offset, len)? {
                    out.extend(projector.project(value));
                }
            }
        }
    }
    Ok(out)
}

/// How the conversation in the first `limit` bytes of `reader` ends
/// (FLOW-FORK-SESSION-R03), looking back from its last entry along its chain.
///
/// A finished reply is an assistant entry the model wrote that ended its
/// turn. What the CLI writes beside a conversation (a local command, a
/// caveat, a compaction summary) is passed over, since no reply follows it.
/// Anything else after the last finished reply says the conversation has
/// moved on since, and the entry nearest the end says how: fixtures
/// 2.1.316/17 and 18 record what `--fork-session` makes of such a record, a
/// made-up tool result and a made-up "No response requested." reply in the
/// copy. A last line left half written counts as a turn being written. With
/// no finished reply anywhere there has been none yet.
pub fn conversation_end_from<R: BufRead + Seek>(
    reader: &mut R,
    limit: u64,
) -> io::Result<ConversationEnd> {
    let Scan { lines, torn } = scan_lines(reader, limit)?;
    let Conversation { chain, .. } = conversation_of(&lines);

    let unreadable_end = matches!(lines.last(), Some(Line::Failed(_)));
    let mut nearest_end = (torn || unreadable_end).then_some(ConversationEnd::Unfinished);
    // Walking back, a command's output comes before its echo.
    let mut local_output_follows = false;
    for line in lines.iter().rev() {
        let Line::Entry {
            offset,
            len,
            chain: fields,
        } = line
        else {
            continue;
        };
        let kind = fields.get("type").and_then(Value::as_str);
        if !on_conversation(&chain, fields) {
            continue;
        }
        if kind == Some("system")
            && fields.get("subtype").and_then(Value::as_str) == Some("local_command")
        {
            local_output_follows = true;
            continue;
        }
        if !matches!(kind, Some("user" | "assistant")) {
            continue;
        }
        let Some(value) = entry_at(reader, *offset, *len)? else {
            continue;
        };
        let end = entry_end(&value, local_output_follows);
        local_output_follows = false;
        match end {
            Some(ConversationEnd::Finished) => {
                return Ok(nearest_end.unwrap_or(ConversationEnd::Finished));
            }
            Some(end) => {
                nearest_end.get_or_insert(end);
            }
            None => {}
        }
    }
    Ok(ConversationEnd::NoReply)
}

/// What one `user` or `assistant` entry says of how the conversation ends;
/// `None` for one the CLI wrote beside the conversation, which a command's
/// echo is only when its output follows.
fn entry_end(value: &Value, local_output_follows: bool) -> Option<ConversationEnd> {
    match RecordEntry::deserialize(value) {
        Ok(RecordEntry::Assistant(reply)) => {
            if reply.message.model == "<synthetic>" {
                // The CLI's stand-in for a reply: "No response requested."
                // after a turn cut short, or an API error's text.
                return Some(ConversationEnd::Interrupted);
            }
            let ended_its_turn = matches!(
                reply.message.stop_reason.as_deref(),
                Some("end_turn" | "stop_sequence" | "max_tokens" | "refusal")
            );
            Some(if ended_its_turn {
                ConversationEnd::Finished
            } else {
                ConversationEnd::Unfinished
            })
        }
        Ok(RecordEntry::User(user)) if user.is_interrupt_marker() => {
            Some(ConversationEnd::Interrupted)
        }
        Ok(RecordEntry::User(user)) if user.is_aside() => None,
        Ok(RecordEntry::User(user)) if user.is_command_echo() && local_output_follows => None,
        // A prompt or a tool result waits for a reply; so does an entry
        // whose shape has drifted, for all Ersa can tell.
        _ => Some(ConversationEnd::Unfinished),
    }
}

/// The conversation's chain among a record's lines.
fn conversation_of(lines: &[Line]) -> Conversation {
    let entries: Vec<&Value> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Entry { chain, .. } => Some(chain),
            Line::Failed(_) => None,
        })
        .collect();
    conversation(&entries)
}

/// A record's lines, read once.
struct Scan {
    lines: Vec<Line>,
    /// The last line stops part way through an entry: the CLI is writing
    /// it, or died writing it.
    torn: bool,
}

/// The first pass over a record: where each entry lies and what its chain
/// needs. See [`project_record_from`].
fn scan_lines<R: BufRead + Seek>(reader: &mut R, limit: u64) -> io::Result<Scan> {
    let mut lines = Vec::new();
    let mut torn = false;
    let mut offset = 0u64;
    while offset < limit {
        let start = offset;
        let mut bytes = Vec::new();
        // Room for the longest entry and its line end (CRLF included).
        let cap = (MAX_ENTRY_BYTES as u64 + 2).min(limit - offset);
        let read = reader.by_ref().take(cap).read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        offset += read as u64;
        let mut terminated = bytes.last() == Some(&b'\n');
        if !terminated && read as u64 == cap && offset < limit {
            // Longer than any entry read: skip the rest of it.
            let (skipped, ended) = skip_line(reader, limit - offset)?;
            offset += skipped;
            terminated = ended;
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        if bytes.len() > MAX_ENTRY_BYTES {
            // A last line still being written is left for the next read,
            // however long it already is.
            if terminated {
                lines.push(Line::Failed(unreadable(
                    &String::from_utf8_lossy(&bytes),
                    format!("a record entry is longer than {MAX_ENTRY_BYTES} bytes"),
                )));
            } else {
                torn = true;
            }
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        if text.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => lines.push(Line::Entry {
                offset: start,
                len: bytes.len(),
                chain: chain_fields(&value),
            }),
            Err(e) if terminated => lines.push(Line::Failed(unreadable(
                &text,
                format!("a record entry is not JSON: {e}"),
            ))),
            // The last line, still being written: left for the next read.
            Err(_) => torn = true,
        }
    }
    Ok(Scan { lines, torn })
}

/// Whether an entry is part of the conversation: one on another branch, or
/// a subagent's, is not (PROTO-CLAUDE-CODE-STREAM-R20). Entries without an
/// id (titles) are let through; they yield nothing.
fn on_conversation(chain: &Option<HashSet<String>>, fields: &Value) -> bool {
    match (chain, uuid(fields)) {
        (Some(chain), Some(id)) => chain.contains(id),
        // In file order, still only the main thread's entries.
        (None, _) => is_main_thread(fields),
        (Some(_), None) => true,
    }
}

/// An entry read whole again, one at a time. It parsed in the first pass, so
/// it parses here; a file cut short since (rewritten by something other than
/// the CLI) loses the entry, not the read.
fn entry_at<R: BufRead + Seek>(
    reader: &mut R,
    offset: u64,
    len: usize,
) -> io::Result<Option<Value>> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; len];
    if let Err(e) = reader.read_exact(&mut bytes) {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(e);
    }
    Ok(serde_json::from_str(&String::from_utf8_lossy(&bytes)).ok())
}

/// Skips the rest of a line too long to read, up to `limit` bytes, and
/// returns how many bytes it skipped and whether it reached the line's end.
fn skip_line<R: BufRead>(reader: &mut R, limit: u64) -> io::Result<(u64, bool)> {
    let mut skipped = 0;
    while skipped < limit {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        let room = usize::try_from(limit - skipped).unwrap_or(usize::MAX);
        let window = &buffer[..buffer.len().min(room)];
        match window.iter().position(|b| *b == b'\n') {
            Some(end) => {
                reader.consume(end + 1);
                skipped += end as u64 + 1;
                return Ok((skipped, true));
            }
            None => {
                let all = window.len();
                reader.consume(all);
                skipped += all as u64;
            }
        }
    }
    Ok((skipped, false))
}

/// Whether an entry belongs to the main conversation rather than to a
/// subagent (`isSidechain`), a teammate (`teamName`) or a progress report.
fn is_main_thread(value: &Value) -> bool {
    value.get("isSidechain").and_then(Value::as_bool) != Some(true)
        && value.get("teamName").is_none()
        && value.get("type").and_then(Value::as_str) != Some("progress")
}

enum Line {
    Entry {
        /// Where the entry's text lies, to read it again to project it.
        offset: u64,
        len: usize,
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
        "subtype",
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
            // A local command's output shows in the transcript as a message
            // does (PROTO-CLAUDE-CODE-STREAM-R23), so a chain may end on it.
            message: matches!(kind, Some("user" | "assistant"))
                || (kind == Some("system") && text("subtype") == Some("local_command")),
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
            // A local command's output, projected as the stream projects the
            // synthetic message it carried (PROTO-CLAUDE-CODE-STREAM-R23).
            // That message's own id is not in the record, so each block is
            // numbered under the entry's `uuid` and its index.
            RecordEntry::System(system) => match system.local_command_output() {
                LocalCommandOutput::NotLocalCommand | LocalCommandOutput::Echo => {
                    return Vec::new();
                }
                LocalCommandOutput::Drifted => {
                    return vec![
                        attribute(protocol_error(
                            "a local_command record entry did not have the expected shape".into(),
                        )),
                        attribute(passthrough(value)),
                    ];
                }
                LocalCommandOutput::Output(texts) => {
                    let id = id.clone().unwrap_or_default();
                    texts
                        .into_iter()
                        .enumerate()
                        .map(|(index, text)| {
                            Event::AssistantText(AssistantText {
                                block_id: format!("{id}/{index}"),
                                text: text.to_string(),
                                is_final: true,
                            })
                        })
                        .collect()
                }
            },
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
    // The CLI's note of where it cached an attached image is not a message:
    // no events, so the entry passes through (fixture 2.1.316/07).
    if user.is_meta == Some(true) && names_cached_images(&user.message.content) {
        return Vec::new();
    }
    let from_cli = user.is_from_cli();
    let content = user.message.content;
    let results = tool_result_events(content.clone());
    if !results.is_empty() {
        return results;
    }
    // Its images and its list of files and folders are its attachments, never
    // its text (PROTO-CLAUDE-CODE-STREAM-R26).
    let has_image = matches!(&content, MessageContent::Blocks(blocks)
        if blocks.iter().any(|b| matches!(b, crate::wire::ContentBlock::Image { .. })));
    let (text, attachments) = match content {
        MessageContent::Text(text) => (Some(text), Vec::new()),
        MessageContent::Blocks(blocks) => crate::attachments::split_blocks(&blocks),
    };
    match (text, attachments.is_empty()) {
        // Images the adapter cannot read leave a prompt with nothing to show,
        // but it was still a turn: it stays in the transcript, empty.
        (None, true) if !has_image => Vec::new(),
        (text, _) => vec![Event::UserMessage(UserMessage {
            text: text.unwrap_or_default(),
            origin: if from_cli {
                MessageOrigin::System
            } else {
                MessageOrigin::Human
            },
            peer: None,
            attachments,
        })],
    }
}

/// Whether `content` is only the CLI's note of where it cached attached
/// images: text alone, every line of it `[Image: source: <path>]`. A note
/// with anything else in it is still shown.
fn names_cached_images(content: &MessageContent) -> bool {
    let cached = |text: &str| {
        !text.is_empty()
            && text
                .lines()
                .all(|line| line.starts_with("[Image: source: ") && line.ends_with(']'))
    };
    match content {
        MessageContent::Text(text) => cached(text),
        MessageContent::Blocks(blocks) => {
            !blocks.is_empty()
                && blocks.iter().all(|block| match block {
                    crate::wire::ContentBlock::Text { text, .. } => cached(text),
                    _ => false,
                })
        }
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
