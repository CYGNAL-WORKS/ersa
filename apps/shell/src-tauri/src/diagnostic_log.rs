//! Opening the diagnostic log (FLOW-READ-RAW-LOG-R13): the file Core
//! writes its warnings and errors to, kept in Ersa's data folder. Only the
//! Shell knows where it is: it sends the output of the Core it launches
//! there ([`CoreLocation::log_file`]). So the Interface asks with no path,
//! and nothing the window sends can make the Shell open any other file.

use std::io;
use std::path::{Path, PathBuf};

use crate::launch::CoreLocation;

/// The diagnostic log's file for the window's Core, or why there is none, in
/// the user's terms. `label` is the endpoint the window was told to attach
/// to: a Core someone else runs there, such as a replay, keeps no log of
/// Ersa's own, so a window attached to one has none to open.
pub fn log_file(
    location: &Result<CoreLocation, String>,
    label: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(label) = label {
        return Err(format!(
            "This build is attached to {label}, so it has no diagnostic log to open."
        ));
    }
    let location = location.as_ref().map_err(Clone::clone)?;
    location
        .log_file
        .clone()
        .ok_or_else(|| "Ersa keeps no diagnostic log for this connection.".to_string())
}

/// Opens `path` with `open`, the system's app for it, once it is there.
/// A file that isn't there yet (Ersa hasn't started its background process
/// on this computer) is said rather than handed on, since the system's
/// opener would answer it with a dialog of its own, or not at all.
pub fn open_file(path: &Path, open: impl FnOnce(&Path) -> io::Result<()>) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("There's no diagnostic log at {} yet.", path.display()));
    }
    open(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Hands `path` to the operating system's opener, which opens it in the app
/// the system uses for its kind of file (Notepad, by default, for a `.log`
/// on Windows). The path is one argument to the program, never a shell
/// command line. Only starting the opener can fail here: none of them says
/// whether the app it chose opened the file.
pub fn system_open(path: &Path) -> io::Result<()> {
    std::process::Command::new(OPENER).arg(path).spawn().map(drop)
}

/// The operating system's opener: given a file or a link, it opens it in
/// the app the system uses for it. Links from the agent's words go through
/// it too.
pub const OPENER: &str = if cfg!(windows) {
    "explorer.exe"
} else if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    // verifies: FLOW-READ-RAW-LOG-R13
    #[test]
    fn the_log_is_the_file_the_launched_core_writes_to() {
        let dir = tempfile::tempdir().expect("a folder");
        let own = CoreLocation::own("daily", dir.path()).expect("a location");
        assert_eq!(log_file(&Ok(own), None), Ok(dir.path().join("core.log")));
    }

    // verifies: FLOW-READ-RAW-LOG-R13
    #[test]
    fn a_window_attached_to_a_label_has_none_and_says_why() {
        let attached = CoreLocation::attach_only("replay-1").expect("a location");
        assert_eq!(
            log_file(&Ok(attached), Some("replay-1")),
            Err("This build is attached to replay-1, so it has no diagnostic log to open.".to_string())
        );
    }

    // verifies: FLOW-READ-RAW-LOG-R13
    #[test]
    fn a_window_with_no_core_says_why_it_has_no_log() {
        let why = "Ersa can't find the folder where it keeps your sessions.".to_string();
        assert_eq!(log_file(&Err(why.clone()), None), Err(why));
    }

    // verifies: FLOW-READ-RAW-LOG-R13
    #[test]
    fn opens_exactly_the_log_file() {
        let dir = tempfile::tempdir().expect("a folder");
        let log = dir.path().join("core.log");
        std::fs::write(&log, "WARN something\n").expect("a log");
        let opened = RefCell::new(Vec::new());
        let answer = open_file(&log, |path| {
            opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        });
        assert_eq!(answer, Ok(()));
        assert_eq!(opened.into_inner(), vec![log]);
    }

    // verifies: FLOW-READ-RAW-LOG-R13
    #[test]
    fn a_log_that_is_not_there_is_not_opened_and_says_so() {
        let dir = tempfile::tempdir().expect("a folder");
        let log = dir.path().join("core.log");
        let mut asked = false;
        let answer = open_file(&log, |_| {
            asked = true;
            Ok(())
        });
        assert_eq!(answer, Err(format!("There's no diagnostic log at {} yet.", log.display())));
        assert!(!asked, "nothing is handed to the opener");
        // Nor a folder in its place.
        std::fs::create_dir(&log).expect("a folder named like the log");
        assert!(open_file(&log, |_| Ok(())).is_err());
    }

    // verifies: FLOW-READ-RAW-LOG-R13
    #[test]
    fn an_opener_that_fails_answers_with_its_reason() {
        let dir = tempfile::tempdir().expect("a folder");
        let log = dir.path().join("core.log");
        std::fs::write(&log, "").expect("a log");
        let answer = open_file(&log, |_| Err(io::Error::new(io::ErrorKind::NotFound, "explorer.exe is missing")));
        assert_eq!(answer, Err(format!("{}: explorer.exe is missing", log.display())));
    }
}
