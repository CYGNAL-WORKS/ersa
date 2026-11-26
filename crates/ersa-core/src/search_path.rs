//! Where Core looks for a backend's command (PROTO-CORE-API, Finding a
//! backend).
//!
//! A program keeps the search path it was started with and hands it on, so
//! a folder an installer added to the Path since is on neither Core's path
//! nor the Shell's. On Windows Core therefore looks along its own path and
//! then along the system's and the user's Path as the system holds them now
//! (PROTO-CORE-API-R148). This module composes that path and looks along
//! it; [`crate::transport::Finder`] does both before each start of a
//! backend's command.

use std::path::{Path, PathBuf};

/// The separator between folders in a search path.
pub use crate::platform::PATH_SEPARATOR as SEPARATOR;
use crate::platform::{is_program, path_key, program_names};

/// How deep a stored variable's value may name further variables before
/// the rest is left as written. Windows stores `windir` as `%SystemRoot%`,
/// one level; a few more cover any sane chain and stop a loop.
const MAX_VARIABLE_DEPTH: usize = 4;

/// The search path as the system holds it now, not as Core inherited it.
///
/// On Windows the registry's system and user Path; on other systems
/// nothing, since there Core looks along the path it was started with only
/// (PROTO-CORE-API, Open questions). Tests give their own.
pub trait StoredEnvironment: Send + Sync {
    /// The stored Path values, the system's first and then the user's, as
    /// written: variables not yet filled in. One that is not set is left
    /// out.
    fn paths(&self) -> std::io::Result<Vec<String>>;

    /// A variable's stored value, the user's before the system's, as
    /// written; `None` when neither holds it.
    fn variable(&self, name: &str) -> Option<String>;
}

/// The search path to look along and to give the started process: the path
/// Core was started with (`inherited`) first, so nothing found before is
/// found somewhere else, then each folder of `stored` with its variables
/// filled in, leaving out a folder already on the path. `None` when there
/// is no path at all.
///
/// A variable is filled from its stored value, as the system holds it now,
/// else from Core's own environment (`own`), which holds the ones Windows
/// never stores; one neither holds is left as written, as Windows leaves it.
pub fn compose(
    inherited: Option<&str>,
    stored: &dyn StoredEnvironment,
    stored_paths: &[String],
    own: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let mut path = inherited.map(str::to_string);
    // The folders already on the path, compared as `same_folder` does.
    let mut seen: Vec<String> = inherited
        .into_iter()
        .flat_map(|p| p.split(SEPARATOR))
        .map(folder_key)
        .collect();
    for value in stored_paths {
        let filled = fill(value, stored, own, 0);
        for folder in filled.split(SEPARATOR).filter(|f| !f.trim().is_empty()) {
            let key = folder_key(folder);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let path = path.get_or_insert_with(String::new);
            if !path.is_empty() && !path.ends_with(SEPARATOR) {
                path.push(SEPARATOR);
            }
            path.push_str(folder);
        }
    }
    path
}

/// `value` with each `%NAME%` replaced by the variable's value, as Windows
/// expands a stored Path. A `%` with no closing one, and a name with no
/// value, stay as written.
fn fill(
    value: &str,
    stored: &dyn StoredEnvironment,
    own: &dyn Fn(&str) -> Option<String>,
    depth: usize,
) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            // No closing `%`: the rest is plain text.
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        let found = (!name.is_empty())
            .then(|| {
                // The stored value first, as the system holds it now; it may
                // itself name variables (`windir` is `%SystemRoot%`), filled
                // the same way, to a limit. Core's own environment holds the
                // ones Windows never stores, such as `USERPROFILE`.
                (depth < MAX_VARIABLE_DEPTH)
                    .then(|| stored.variable(name))
                    .flatten()
                    .map(|v| fill(&v, stored, own, depth + 1))
                    .or_else(|| own(name))
            })
            .flatten();
        match found {
            Some(v) => {
                out.push_str(&v);
                rest = &after[end + 1..];
            }
            None => {
                // Left as written; the closing `%` may open the next name,
                // as in `%NOPE%PATH%`, so only the first `%` is passed.
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A folder as compared for repeats: as [`path_key`] compares two spellings
/// of one folder (on Windows without regard to letter case, which its file
/// system ignores), without surrounding blanks.
fn folder_key(folder: &str) -> String {
    path_key(folder.trim())
}

/// The file the host would start for `program` along `path`, as the command
/// prompt or a shell finds it; `None` when no folder holds it.
///
/// A program with a folder in its name is taken as named, if it is a file.
/// On Windows a bare name is tried as given when it has an extension, then
/// with each extension in `PATHEXT`, since many command-line tools, Claude
/// Code's npm install among them, are `.cmd` scripts rather than `.exe`
/// files. On other systems the file must be executable.
pub fn find_program(program: &str, path: Option<&str>) -> Option<PathBuf> {
    let as_given = Path::new(program);
    if as_given.components().count() != 1 {
        return is_program(as_given).then(|| as_given.to_path_buf());
    }
    let names = program_names(program);
    path?
        .split(SEPARATOR)
        .map(str::trim)
        .filter(|folder| !folder.is_empty())
        .flat_map(|folder| names.iter().map(move |name| Path::new(folder).join(name)))
        .find(|candidate| is_program(candidate))
}

/// The stored search path of the host Core runs on: the registry's on
/// Windows ([`crate::platform::stored_paths`]), nothing elsewhere.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemStoredEnvironment;

impl StoredEnvironment for SystemStoredEnvironment {
    fn paths(&self) -> std::io::Result<Vec<String>> {
        crate::platform::stored_paths()
    }

    fn variable(&self, name: &str) -> Option<String> {
        crate::platform::stored_variable(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Stored values of the test's own.
    #[derive(Default)]
    struct Stored {
        variables: HashMap<String, String>,
    }

    impl StoredEnvironment for Stored {
        fn paths(&self) -> std::io::Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn variable(&self, name: &str) -> Option<String> {
            self.variables.get(name).cloned()
        }
    }

    fn joined(folders: &[&str]) -> String {
        folders.join(&SEPARATOR.to_string())
    }

    fn own(name: &str) -> Option<String> {
        match name {
            "HOME_DIR" => Some("/home/me".to_string()),
            "OWN_ONLY" => Some("/home/me".to_string()),
            _ => None,
        }
    }

    // verifies: PROTO-CORE-API-R148
    #[test]
    fn the_path_core_was_started_with_comes_first_then_the_system_then_the_user() {
        let composed = compose(
            Some(&joined(&["/a", "/b"])),
            &Stored::default(),
            &[joined(&["/sys1", "/sys2"]), joined(&["/user1"])],
            &own,
        );
        assert_eq!(
            composed.as_deref(),
            Some(joined(&["/a", "/b", "/sys1", "/sys2", "/user1"]).as_str())
        );
    }

    // verifies: PROTO-CORE-API-R148
    #[test]
    fn a_folder_already_on_the_path_is_not_added_again() {
        let composed = compose(
            Some(&joined(&["/a", "/b/"])),
            &Stored::default(),
            &[joined(&["/b", "/c", "", "/a"]), joined(&["/c/", "/d"])],
            &own,
        );
        assert_eq!(
            composed.as_deref(),
            Some(joined(&["/a", "/b/", "/c", "/d"]).as_str())
        );
    }

    // verifies: PROTO-CORE-API-R148
    #[test]
    fn variables_are_filled_from_the_stored_values_then_cores_own_environment() {
        let stored = Stored {
            variables: HashMap::from([
                ("TOOLS".to_string(), "%HOME_DIR%/tools".to_string()),
                // The stored value wins over Core's own, which is older.
                ("HOME_DIR".to_string(), "/stored/home".to_string()),
            ]),
        };
        let composed = compose(
            None,
            &stored,
            &[joined(&[
                "%HOME_DIR%/bin",
                "%TOOLS%/x",
                "%OWN_ONLY%/own",
                "%NOPE%/y",
                "50%off",
            ])],
            &own,
        );
        assert_eq!(
            composed.as_deref(),
            Some(
                joined(&[
                    "/stored/home/bin",
                    "/stored/home/tools/x",
                    "/home/me/own",
                    "%NOPE%/y",
                    "50%off"
                ])
                .as_str()
            )
        );
    }

    #[test]
    fn a_variable_that_names_itself_stops_at_the_limit() {
        let stored = Stored {
            variables: HashMap::from([("LOOP".to_string(), "%LOOP%x".to_string())]),
        };
        let composed = compose(None, &stored, &["%LOOP%".to_string()], &own).unwrap();
        assert!(composed.starts_with("%LOOP%"), "{composed}");
    }

    #[test]
    fn no_path_at_all_is_none() {
        assert_eq!(compose(None, &Stored::default(), &[], &own), None);
    }
}
