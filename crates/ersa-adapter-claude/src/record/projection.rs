//! The projection of session record entries to Ersa events.
//!
//! A `user` or `assistant` entry's `message` is the message the live stream
//! carried with the same `uuid`, so it goes through the same functions as the
//! stream does and yields the same events (PROTO-CLAUDE-CODE-STREAM-R15). What
//! only the live stream carries (text as it arrives, permission prompts, turn
//! ends, cost, rate limits) is not in the record, so neither are the events it
//! yields: the record has no turns, only messages.

use std::collections::HashMap;

use ersa_protocol::events::{
    BackendPassthrough, ErrorCode, ErrorEvent, Event, MessageOrigin, UserMessage,
};
use serde::Deserialize;
use serde_json::Value;

use crate::BACKEND;
use crate::projector::{kind_of, plain_text, reply_events, tool_result_events};
use crate::wire::{MessageContent, RecordEntry, RecordUser};

/// One event projected from a record entry.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordEvent {
    /// The entry's `uuid`: the backend's message id, on which record and live
    /// input are merged. `None` for an entry that could not be read.
    pub message_id: Option<String>,
    /// When the CLI wrote the entry, as the entry gives it.
    pub ts: Option<String>,
    pub event: Event,
}

/// Projects one session record, entry by entry, in order. Make a fresh one
/// per pass: like the stream's projector it numbers each message's content
/// blocks across the entries that carry them.
#[derive(Debug, Default)]
pub struct RecordProjector {
    blocks_seen: HashMap<String, u64>,
}

impl RecordProjector {
    pub fn new() -> Self {
        RecordProjector::default()
    }

    /// The events of one record line. Entries that carry no message (titles,
    /// snapshots, attachments, and whatever else the CLI keeps) yield none.
    pub fn project(&mut self, line: &str) -> Vec<RecordEvent> {
        // The CLI ends every entry with a newline; an empty line is no entry.
        if line.trim().is_empty() {
            return Vec::new();
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(e) => {
                // A projection failure is an error and the line, and the
                // pass carries on (PROTO-AGENT-EVENTS-R10).
                return vec![
                    unattributed(protocol_error(format!("a record entry is not JSON: {e}"))),
                    unattributed(Event::BackendPassthrough(BackendPassthrough {
                        backend: BACKEND.into(),
                        kind: "unparsed".into(),
                        payload: Value::String(line.to_string()),
                    })),
                ];
            }
        };
        let id = value.get("uuid").and_then(Value::as_str).map(String::from);
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
                // The main record holds the main thread only.
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

fn unattributed(event: Event) -> RecordEvent {
    RecordEvent {
        message_id: None,
        ts: None,
        event,
    }
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
