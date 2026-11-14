//! How many messages a live session's CLI holds behind its running turn
//! (PROTO-CLAUDE-CODE-STREAM-R46).
//!
//! A message written to the CLI during a turn waits in the CLI and runs as
//! the next turn (PROTO-CLAUDE-CODE-STREAM, Sequencing). The CLI keeps no
//! count of them anywhere Ersa can ask, but its record says what happens to
//! each message: a `queue-operation` entry with `operation` `enqueue` when
//! the CLI takes it, and one with `dequeue` when its turn is about to start,
//! or, when the CLI folds it into the running turn at the turn's next model
//! call (a turn that uses tools), one with `remove` and `reason`
//! `absorbed_mid_turn` (2.1.316/51-message-during-tool-turn). So the messages
//! waiting are the `enqueue` entries less the `dequeue` and `remove`
//! entries, counting only those written since the
//! adapter started this CLI process: an earlier process's entries are not
//! this one's (one killed with a message waiting leaves an `enqueue` that is
//! never matched).

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The running count of one CLI process's queue entries. It reads the record
/// a piece at a time: each read goes on from where the last one stopped, so
/// a long record is read whole only once.
#[derive(Debug, Clone)]
pub struct WaitingCount {
    /// When the adapter started the CLI process: entries stamped earlier
    /// belong to an earlier process.
    since: OffsetDateTime,
    /// How many bytes of the record have been read, always up to the end of
    /// a complete line: the CLI may be part way through writing the last one.
    read_to: u64,
    enqueued: u32,
    /// `dequeue` and `remove` entries: messages the CLI no longer holds.
    taken: u32,
}

impl WaitingCount {
    /// A count for a CLI process started at `since`, before any of its
    /// record is read.
    pub fn new(since: OffsetDateTime) -> Self {
        WaitingCount {
            since,
            read_to: 0,
            enqueued: 0,
            taken: 0,
        }
    }

    /// How many messages the CLI holds, as of the last read.
    pub fn waiting(&self) -> u32 {
        // Never below zero, whatever the record holds.
        self.enqueued.saturating_sub(self.taken)
    }

    /// How many messages the CLI has taken since it started, as of the last
    /// read: grows when the CLI writes the `enqueue` for a message just
    /// handed over.
    pub fn enqueued(&self) -> u32 {
        self.enqueued
    }

    /// Reads what the record at `path` has gained since the last read and
    /// returns the messages waiting. A record shorter than what was already
    /// read is a new file, so it is counted again from its start.
    pub fn read(&mut self, path: &Path) -> io::Result<u32> {
        let mut file = File::open(path)?;
        if file.metadata()?.len() < self.read_to {
            *self = WaitingCount::new(self.since);
        }
        file.seek(SeekFrom::Start(self.read_to))?;
        self.read_from(BufReader::new(file))
    }

    /// Reads the record's lines from `reader`, which starts where the last
    /// read stopped, and returns the messages waiting.
    pub fn read_from(&mut self, mut reader: impl BufRead) -> io::Result<u32> {
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            // A line without its newline is still being written; the next
            // read starts at it again.
            if read == 0 || line.last() != Some(&b'\n') {
                break;
            }
            self.read_to += read as u64;
            self.count(&line);
        }
        Ok(self.waiting())
    }

    /// Counts one record line, if it is a queue entry of this process.
    fn count(&mut self, line: &[u8]) {
        // Most lines are long messages; only parse the few that can be queue
        // entries.
        const KIND: &[u8] = b"queue-operation";
        if !line.windows(KIND.len()).any(|window| window == KIND) {
            return;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(line) else {
            return;
        };
        if entry["type"] != "queue-operation" {
            return;
        }
        let stamped = entry["timestamp"]
            .as_str()
            .and_then(|stamp| OffsetDateTime::parse(stamp, &Rfc3339).ok());
        // An entry whose time does not read cannot be placed in this
        // process, so it is not counted.
        if !stamped.is_some_and(|stamped| stamped >= self.since) {
            return;
        }
        match entry["operation"].as_str() {
            Some("enqueue") => self.enqueued = self.enqueued.saturating_add(1),
            Some("dequeue" | "remove") => self.taken = self.taken.saturating_add(1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Cursor, Write};

    use super::*;

    /// A session-record fixture of 2.1.316, by name.
    fn fixture_record(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/briefs/protocol/fixtures/claude-code/2.1.316/record")
            .join(format!("{name}.jsonl"));
        // Git may check text files out with CRLF line ends on Windows.
        fs::read_to_string(path).unwrap().replace("\r\n", "\n")
    }

    /// Fixture 22's record: a message written while the first turn runs,
    /// with its queue entries kept verbatim.
    fn record_22() -> String {
        fixture_record("22-message-during-turn")
    }

    /// The record up to and including the line holding `marker`.
    fn up_to(record: &str, marker: &str) -> String {
        let mut kept = String::new();
        for line in record.split_inclusive('\n') {
            kept.push_str(line);
            if line.contains(marker) {
                return kept;
            }
        }
        panic!("no line holds {marker}");
    }

    fn at(stamp: &str) -> OffsetDateTime {
        OffsetDateTime::parse(stamp, &Rfc3339).unwrap()
    }

    fn count(record: &str, since: &str) -> u32 {
        WaitingCount::new(at(since))
            .read_from(Cursor::new(record.as_bytes()))
            .unwrap()
    }

    /// The second message's `enqueue`, written while the first turn ran.
    const MID_TURN_ENQUEUE: &str = r#""enqueue","timestamp":"2026-11-14T01:04:38.253Z""#;

    // verifies: PROTO-CLAUDE-CODE-STREAM-R46
    #[test]
    fn a_message_written_during_a_turn_waits_until_its_dequeue() {
        let record = record_22();
        // The process started before the first message (recorded at
        // 01:04:30.929): the first message's pair cancels out, and the
        // second waits once its `enqueue` is written.
        let started = "2026-11-14T01:04:30.929Z";
        assert_eq!(count(&up_to(&record, MID_TURN_ENQUEUE), started), 1);
        // Its `dequeue` comes when the first turn ends: none waits then.
        assert_eq!(count(&record, started), 0);
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R46
    #[test]
    fn a_message_folded_into_the_running_turn_no_longer_waits() {
        // Fixture 51: the turn uses tools, and the CLI hands the waiting
        // message to the model at its next call, writing `remove` with
        // `absorbed_mid_turn` instead of a `dequeue`.
        let record = fixture_record("51-message-during-tool-turn");
        let started = "2026-11-14T20:02:30.702Z";
        assert_eq!(
            count(
                &up_to(
                    &record,
                    r#""enqueue","timestamp":"2026-11-14T20:02:35.488Z""#
                ),
                started
            ),
            1
        );
        assert_eq!(count(&record, started), 0);
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R46
    #[test]
    fn an_earlier_process_entries_are_not_counted() {
        // A process killed with a message waiting leaves an `enqueue` that
        // no `dequeue` matches. A process started after it holds nothing.
        let record = up_to(&record_22(), MID_TURN_ENQUEUE);
        assert_eq!(count(&record, "2026-11-14T01:05:00.000Z"), 0);
        // One started between the first message's pair and the waiting
        // message counts the waiting message alone.
        assert_eq!(count(&record, "2026-11-14T01:04:35.000Z"), 1);
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R46
    #[test]
    fn each_read_goes_on_from_the_last() {
        let record = record_22();
        let first = up_to(&record, MID_TURN_ENQUEUE);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.jsonl");
        fs::write(&path, &first).unwrap();

        let mut waiting = WaitingCount::new(at("2026-11-14T01:04:30.929Z"));
        assert_eq!(waiting.read(&path).unwrap(), 1);
        assert_eq!(waiting.enqueued(), 2);
        // A line still being written is left for the next read.
        let rest = &record[first.len()..];
        let (written, unfinished) = rest.split_at(rest.find(r#""dequeue""#).unwrap());
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(written.as_bytes()).unwrap();
        file.write_all(unfinished.split('\n').next().unwrap().as_bytes())
            .unwrap();
        assert_eq!(waiting.read(&path).unwrap(), 1);
        // Once the line is whole, its `dequeue` counts, once.
        fs::write(&path, &record).unwrap();
        assert_eq!(waiting.read(&path).unwrap(), 0);
        assert_eq!(waiting.read(&path).unwrap(), 0);
        assert_eq!(waiting.enqueued(), 2);
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R46
    #[test]
    fn a_record_shorter_than_what_was_read_is_counted_again() {
        let record = record_22();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("record.jsonl");
        fs::write(&path, &record).unwrap();
        let mut waiting = WaitingCount::new(at("2026-11-14T01:04:30.929Z"));
        assert_eq!(waiting.read(&path).unwrap(), 0);
        fs::write(&path, up_to(&record, MID_TURN_ENQUEUE)).unwrap();
        assert_eq!(waiting.read(&path).unwrap(), 1);
    }
}
