//! Deleting a folder in steps that can be called off between any two of
//! them, for the user's Delete: Core's deletion of a workspace's folder and
//! an adapter's deletion of a session's record (PROTO-CORE-API-R91).
//!
//! `std::fs::remove_dir_all` makes every deletion in one call, which nothing
//! can stop part way, so a delete Core had given up on could go on deleting.
//! [`remove_tree`] walks the folder itself and asks before every single
//! deletion whether it has been called off.

use std::fs;
use std::io;
use std::path::Path;

/// How [`remove_tree`] ended without an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    /// Everything is gone, or was already.
    Removed,
    /// It was called off, and began no deletion after that; what it had not
    /// reached is still there.
    CalledOff,
}

/// Deletes `path` and, when it is a folder, everything in it, asking `stop`
/// before each deletion and beginning none once `stop` says yes. Only a
/// single deletion already begun when the work is called off may still
/// finish.
///
/// A link is never followed: a symbolic link, or on Windows a junction, is
/// removed as a link, wherever it is, the root included (as `remove_dir_all`
/// removes it), and what it leads to is left alone. Each entry is looked at
/// without following it before it is entered. A process that swaps a folder
/// for a link between that look and the listing could lead the walk outside;
/// only a process running as the user can write in the folders this deletes,
/// and it can already delete what such a link would lead to.
///
/// A root already gone, and an entry that goes while the walk runs, count
/// as removed. A read-only file is deleted too, as `remove_dir_all` deletes
/// it (on Windows, `remove_file` ignores the read-only attribute).
pub fn remove_tree(path: &Path, stop: &dyn Fn() -> bool) -> io::Result<Removal> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Removal::Removed),
        Err(e) => return Err(e),
    };
    if !metadata.is_dir() {
        // A file or a link (`symlink_metadata` reports a link as a link,
        // never as the folder it leads to).
        return remove_entry(path, &metadata, stop);
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Removal::Removed),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        // Recursion depth is the folder's depth, which the file system
        // itself bounds well below what a thread's stack holds.
        if remove_tree(&entry.path(), stop)? == Removal::CalledOff {
            return Ok(Removal::CalledOff);
        }
    }
    if stop() {
        return Ok(Removal::CalledOff);
    }
    ignore_gone(fs::remove_dir(path))
}

/// Deletes the file or link at `path`, whose own metadata (not its
/// target's) is `metadata`, unless the work has been called off.
fn remove_entry(
    path: &Path,
    metadata: &fs::Metadata,
    stop: &dyn Fn() -> bool,
) -> io::Result<Removal> {
    if stop() {
        return Ok(Removal::CalledOff);
    }
    let removed = match fs::remove_file(path) {
        // A link to a folder is a folder entry on Windows, which only
        // `remove_dir` takes; it removes the link, never what it leads to,
        // and elsewhere refuses a link outright.
        Err(e) if metadata.is_symlink() => fs::remove_dir(path).map_err(|_| e),
        other => other,
    };
    ignore_gone(removed)
}

/// A deletion whose entry was already gone succeeded.
fn ignore_gone(result: io::Result<()>) -> io::Result<Removal> {
    match result {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(Removal::Removed),
    }
}
