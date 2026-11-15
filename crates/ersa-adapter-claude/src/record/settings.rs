//! What a session's record says of its model, effort and permission mode
//! (PROTO-CLAUDE-CODE-STREAM-R57 to R59).
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

use ersa_adapter::{Recorded, RecordedSettings, RelaunchSettings, Requested};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::listing::{CHUNK, entries};
use crate::projector::SYNTHETIC_MODEL;
use crate::wire::{Extra, RecordEntry};

/// The settings `entries` hold, each with when its turn began.
pub fn settings(entries: &[RecordEntry]) -> RecordedSettings {
    let prompts = prompts(entries);
    let replies = replies(entries, &prompts);
    // `max_by_key` keeps the last of equals, so of two entries stamped alike
    // the one written later wins.
    let mode = prompts.iter().max_by_key(|(at, _)| *at);
    let reply = replies.iter().max_by_key(|reply| reply.at);
    RecordedSettings {
        model: reply.map(Reply::model),
        effort: reply.and_then(Reply::effort),
        permission_mode: mode.map(|(at, mode)| recorded(mode, Some(*at))),
    }
}

/// For each value Core gave a time for, what the first turn that began
/// after that time ran with (PROTO-CLAUDE-CODE-STREAM-R60). A turn begins at
/// its prompt, which carries the mode; the model and effort are those of the
/// earliest reply whose turn began after the time, so a turn interrupted
/// before any reply gives its mode but no model.
fn first_after(entries: &[RecordEntry], requested: Requested) -> RecordedSettings {
    let prompts = prompts(entries);
    let replies = replies(entries, &prompts);
    // `min_by_key` keeps the first of equals: of two entries stamped alike,
    // the one written first.
    let first_reply = |request: Option<OffsetDateTime>| {
        let request = request?;
        replies
            .iter()
            .filter(|reply| reply.began.is_some_and(|began| began > request))
            .min_by_key(|reply| reply.at)
    };
    let first_mode = requested.permission_mode.and_then(|request| {
        prompts
            .iter()
            .filter(|(began, _)| *began > request)
            .min_by_key(|(began, _)| *began)
    });
    RecordedSettings {
        model: first_reply(requested.model).map(Reply::model),
        effort: first_reply(requested.effort).and_then(Reply::effort),
        permission_mode: first_mode.map(|(at, mode)| recorded(mode, Some(*at))),
    }
}

fn recorded(value: &str, turn_began: Option<OffsetDateTime>) -> Recorded {
    Recorded {
        value: value.to_string(),
        turn_began,
    }
}

/// One of the session's own replies that came from the API.
struct Reply<'a> {
    /// The reply's `timestamp`.
    at: OffsetDateTime,
    /// When its turn began: the latest prompt not after it, `None` when no
    /// such prompt was read.
    began: Option<OffsetDateTime>,
    model: &'a str,
    effort: Option<&'a str>,
}

impl Reply<'_> {
    fn model(&self) -> Recorded {
        recorded(self.model, self.began)
    }

    fn effort(&self) -> Option<Recorded> {
        Some(recorded(self.effort?, self.began))
    }
}

/// The session's own replies in `entries`, never a subagent's or one the CLI
/// wrote itself (`<synthetic>`), each with when its turn began.
fn replies<'a>(entries: &'a [RecordEntry], prompts: &[(OffsetDateTime, &str)]) -> Vec<Reply<'a>> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            RecordEntry::Assistant(reply)
                if reply.message.model != SYNTHETIC_MODEL && !in_sidechain(&reply.extra) =>
            {
                let at = stamp(reply.timestamp.as_deref())?;
                let began = prompts
                    .iter()
                    .map(|(began, _)| *began)
                    .filter(|began| *began <= at)
                    .max();
                Some(Reply {
                    at,
                    began,
                    model: &reply.message.model,
                    effort: reply.effort.as_deref(),
                })
            }
            _ => None,
        })
        .collect()
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
/// record's start. A value Core requested is settled once a turn that began
/// at or before the request has been read: every turn after the request has
/// then been read, the first of them included (PROTO-CLAUDE-CODE-STREAM-R60),
/// and every turn before it began earlier still, so it can't count. A value
/// Core never requested is settled by finding it with its turn's start.
pub fn read_back(path: &Path, requested: Requested) -> io::Result<RelaunchSettings> {
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
        let settled = |found: bool, request: Option<OffsetDateTime>| match request {
            Some(request) => prompts.iter().any(|(began, _)| *began <= request),
            None => found,
        };
        // Model and effort come from the same reply.
        let reply_found = found.model.as_ref().is_some_and(|m| m.turn_began.is_some());
        let all_settled = settled(reply_found, requested.model)
            && settled(reply_found, requested.effort)
            && settled(found.permission_mode.is_some(), requested.permission_mode);
        if start == 0 || all_settled {
            return Ok(RelaunchSettings {
                latest: found,
                first: first_after(&read, requested),
            });
        }
        window = window.saturating_mul(2);
    }
}
