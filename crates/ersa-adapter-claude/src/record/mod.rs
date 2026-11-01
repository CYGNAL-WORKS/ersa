//! Claude Code's own session record, which is a session's history (ADR 0017,
//! PROTO-CLAUDE-CODE-STREAM, Session record).
//!
//! The CLI appends each session to
//! `<claude home>/projects/<encoded cwd>/<session id>.jsonl` as it runs,
//! including turns taken outside Ersa, and lists the processes that have a
//! session open in `<claude home>/sessions/<pid>.json`. `<claude home>` is
//! normally `~/.claude` on the session's host; callers pass it in.
//!
//! - [`project_record`] turns a record into Ersa events, through the
//!   same projection of messages as the live stream.
//! - [`list_sessions`] lists a directory's sessions, reading only the head and
//!   tail of each record.
//! - [`live_session`] says whether a running CLI has a session open.
//!
//! These read local files. A remote host's record needs reading through its
//! transport, which has no file access yet.

mod listing;
mod projection;
mod registry;

use std::path::{Path, PathBuf};

pub use listing::{SessionList, SessionSummary, list_sessions};
pub use projection::{RecordEvent, project_record};
pub use registry::{LiveCheck, LiveSession, live_session};

/// The longest encoded directory name the CLI uses whole; a longer one is cut
/// and given a hash of the full path. Verified on 2.1.316: a 222-character
/// working directory was recorded under its first 200 encoded characters,
/// `-`, and exactly the hash [`path_hash`] computes.
const MAX_ENCODED_LEN: usize = 200;

/// The name of the directory under `projects/` that holds the sessions of
/// working directory `cwd`: every character other than an ASCII letter or
/// digit becomes `-` (`D:\Projects\ersa` is `D--Projects-ersa`).
///
/// The CLI is JavaScript and counts in UTF-16 code units, so a character
/// outside the Basic Multilingual Plane (an emoji, say) becomes two dashes.
pub fn encoded_cwd(cwd: &str) -> String {
    let mut encoded = String::with_capacity(cwd.len());
    for c in cwd.chars() {
        if c.is_ascii_alphanumeric() {
            encoded.push(c);
        } else {
            for _ in 0..c.len_utf16() {
                encoded.push('-');
            }
        }
    }
    if encoded.len() <= MAX_ENCODED_LEN {
        return encoded;
    }
    // Every character is ASCII by now, so cutting at a byte index is safe.
    encoded.truncate(MAX_ENCODED_LEN);
    format!("{encoded}-{}", path_hash(cwd))
}

/// The hash the CLI appends to a cut directory name: the 32-bit string hash
/// JavaScript code commonly uses (`h = h * 31 + unit`, over UTF-16 code units,
/// wrapping), its absolute value in base 36.
fn path_hash(cwd: &str) -> String {
    let hash = cwd.encode_utf16().fold(0i32, |h, unit| {
        h.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    // `unsigned_abs` keeps i32::MIN's magnitude, as JavaScript's `Math.abs`
    // does on its doubles.
    let mut n = hash.unsigned_abs();
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(char::from_digit(n % 36, 36).expect("below 36"));
        n /= 36;
    }
    digits.iter().rev().collect()
}

/// The directory holding the session records of working directory `cwd`.
///
/// `cwd` must be spelled as the CLI spelled it, which is the `cwd` its
/// `system/init` reports: a canonicalised path (`\?\D:\...`), another
/// drive-letter case or a trailing separator encodes to another directory,
/// and that directory simply has no sessions.
pub fn project_dir(claude_home: &Path, cwd: &str) -> PathBuf {
    claude_home.join("projects").join(encoded_cwd(cwd))
}

/// The session record of session `session_id`, started in `cwd`.
pub fn record_path(claude_home: &Path, cwd: &str, session_id: &str) -> PathBuf {
    project_dir(claude_home, cwd).join(format!("{session_id}.jsonl"))
}

/// Whether `id` has the shape of a Claude Code session id: a UUID in its
/// hyphenated form. Ids read from file names, or handed to `--resume`, are
/// checked with this before use.
pub fn is_session_id(id: &str) -> bool {
    let groups: Vec<&str> = id.split('-').collect();
    let lengths = [8, 4, 4, 4, 12];
    groups.len() == lengths.len()
        && groups
            .iter()
            .zip(lengths)
            .all(|(group, len)| group.len() == len && group.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_like_the_cli() {
        assert_eq!(encoded_cwd(r"D:\Projects\ersa"), "D--Projects-ersa");
        assert_eq!(encoded_cwd("/home/jo/my repo"), "-home-jo-my-repo");
        assert_eq!(encoded_cwd("C:/é"), "C---");
        // One emoji is two UTF-16 code units, so two dashes.
        assert_eq!(encoded_cwd("a😀b"), "a--b");
    }

    #[test]
    fn cuts_long_names_and_adds_the_path_hash() {
        let long = format!("C:/{}", "a".repeat(300));
        let encoded = encoded_cwd(&long);
        let (cut, hash) = encoded.split_at(MAX_ENCODED_LEN);
        assert_eq!(cut, &format!("C--{}", "a".repeat(197)));
        assert_eq!(hash, format!("-{}", path_hash(&long)));
        assert!(encoded_cwd(&"a".repeat(200)).len() == 200);
    }

    #[test]
    fn hashes_like_javascript() {
        // Values computed with the same function in Node.js:
        // (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = (h << 5) - h + s.charCodeAt(i) | 0; return Math.abs(h).toString(36) }
        assert_eq!(path_hash(""), "0");
        assert_eq!(path_hash("a"), "2p");
        assert_eq!(path_hash("hello world"), "to5x38");
        assert_eq!(path_hash(&format!("C:/{}", "a".repeat(300))), "2el7y0");
    }

    #[test]
    fn session_ids_are_uuids() {
        assert!(is_session_id("d34f0c1b-64ee-437e-a4e3-3487df29a506"));
        for bad in [
            "",
            "--resume",
            "d34f0c1b-64ee-437e-a4e3-3487df29a50",
            "d34f0c1b-64ee-437e-a4e3-3487df29a5066",
            "d34f0c1b_64ee-437e-a4e3-3487df29a506",
            "g34f0c1b-64ee-437e-a4e3-3487df29a506",
            "../../etc/passwd",
        ] {
            assert!(!is_session_id(bad), "{bad}");
        }
    }
}
