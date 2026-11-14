//! What a session's record says of its model, effort and permission mode
//! (PROTO-CLAUDE-CODE-STREAM-R47).
//!
//! The model and effort are those of the main thread's latest reply that
//! came from the API, and the permission mode that of the latest prompt:
//! the CLI writes the mode on each prompt it starts a turn with, and on no
//! other entry (tool results, meta and local-command entries carry none).
//! A prompt is stamped when its turn starts, even one written while the
//! turn before it ran (fixture 2.1.316/22), so a prompt's `timestamp` is
//! when its turn began, and a reply's turn began at the latest prompt not
//! after it. Entries are not written in time order, so "latest" is by
//! `timestamp`, never by place in the file.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use ersa_adapter::{Recorded, RecordedSettings, Requested};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::listing::{CHUNK, entries};
use crate::projector::SYNTHETIC_MODEL;
use crate::wire::{Extra, RecordEntry};

/// The settings `entries` hold, each with when its turn began.
pub fn settings(entries: &[RecordEntry]) -> RecordedSettings {
    let prompts = prompts(entries);
    // `max_by_key` keeps the last of equals, so of two entries stamped alike
    // the one written later wins.
    let mode = prompts.iter().max_by_key(|(at, _)| *at);
    let reply = entries
        .iter()
        .filter_map(|entry| match entry {
            RecordEntry::Assistant(reply)
                if reply.message.model != SYNTHETIC_MODEL && !in_sidechain(&reply.extra) =>
            {
                Some((stamp(reply.timestamp.as_deref())?, reply))
            }
            _ => None,
        })
        .max_by_key(|(at, _)| *at);
    let reply_began = reply.and_then(|(at, _)| {
        prompts
            .iter()
            .map(|(began, _)| *began)
            .filter(|began| *began <= at)
            .max()
    });
    let recorded = |value: &str, turn_began| Recorded {
        value: value.to_string(),
        turn_began,
    };
    RecordedSettings {
        model: reply.map(|(_, reply)| recorded(&reply.message.model, reply_began)),
        effort: reply.and_then(|(_, reply)| Some(recorded(reply.effort.as_deref()?, reply_began))),
        permission_mode: mode.map(|(at, mode)| recorded(mode, Some(*at))),
    }
}

/// Each prompt's time, which is when its turn began, and the permission mode
/// it carries.
fn prompts(entries: &[RecordEntry]) -> Vec<(OffsetDateTime, &str)> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            RecordEntry::User(user) if !in_sidechain(&user.extra) => Some((
                stamp(user.timestamp.as_deref())?,
                user.permission_mode.as_deref()?,
            )),
            _ => None,
        })
        .collect()
}

/// A subagent's entries are its own, not the session's.
fn in_sidechain(extra: &Extra) -> bool {
    extra.get("isSidechain").and_then(Value::as_bool) == Some(true)
}

/// An entry's `timestamp` as a time; `None` when it has none that reads.
fn stamp(timestamp: Option<&str>) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(timestamp?, &Rfc3339).ok()
}

/// The settings of the record at `path`, for a relaunch: read back from the
/// end, a growing chunk at a time, until each value is settled or the
/// record's start. A value is settled once it is found with its turn's
/// start, or once a turn that began at or before Core's request for it has
/// been read: every turn before that began earlier still, so Core's request
/// stands. A value Core never requested is settled only by finding it.
pub fn read_back(path: &Path, requested: Requested) -> io::Result<RecordedSettings> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut window = CHUNK;
    loop {
        // One byte early, as for a listing's tail: a line is complete only
        // when the byte before it ends the line before.
        let start = len.saturating_sub(window + 1);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        (&mut file).take(len - start).read_to_end(&mut bytes)?;
        let read = entries(&bytes, start > 0, false);
        let found = settings(&read);
        let prompts = prompts(&read);
        let reached = |request: Option<OffsetDateTime>| {
            request.is_some_and(|request| prompts.iter().any(|(began, _)| *began <= request))
        };
        // Model and effort come from the same reply.
        let reply_found = found.model.as_ref().is_some_and(|m| m.turn_began.is_some());
        let settled = (reply_found || reached(requested.model))
            && (reply_found || reached(requested.effort))
            && (found.permission_mode.is_some() || reached(requested.permission_mode));
        if start == 0 || settled {
            return Ok(found);
        }
        window = window.saturating_mul(2);
    }
}
