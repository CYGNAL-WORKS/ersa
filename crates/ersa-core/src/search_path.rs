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

/// A variable's value by name, or `None`.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The search path to look along and to give the started process: the path
/// Core was started with (`inherited`) first, so nothing found before is
/// found somewhere else, then each folder of `stored_paths` with its
/// variables filled in, leaving out a folder already on the path. `None`
/// when there is no path at all. With nothing to add, `inherited` comes
/// back exactly as given.
///
/// A variable is filled from its stored value (`stored`), as the system
/// holds it now, else from Core's own environment (`own`), which holds the
/// ones Windows never stores; one neither holds is left as written, as
/// Windows leaves it. Folders are split and joined as the host does
/// ([`std::env::split_paths`]), so a quoted Windows entry is the folder
/// inside its quotes.
pub fn compose(
    inherited: Option<&str>,
    stored_paths: &[String],
    stored: Lookup,
    own: Lookup,
) -> Option<String> {
    let mut folders: Vec<PathBuf> = inherited
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    let mut seen: Vec<String> = folders.iter().map(|f| folder_key(f)).collect();
    let mut added = false;
    for value in stored_paths {
        let filled = fill(value, stored, own, 0);
        for folder in std::env::split_paths(&filled) {
            let key = folder_key(&folder);
            // An empty entry, a repeat, or a folder the host could not
            // write in a path (a stray quote) is left out.
            if key.is_empty() || seen.contains(&key) || std::env::join_paths([&folder]).is_err() {
                continue;
            }
            seen.push(key);
            folders.push(folder);
            added = true;
        }
    }
    if !added {
        return inherited.map(str::to_string);
    }
    match std::env::join_paths(&folders) {
        Ok(path) => Some(path.to_string_lossy().into_owned()),
        // Only an inherited entry can fail here; give that path as it was.
        Err(_) => inherited.map(str::to_string),
    }
}

/// `value` with each `%NAME%` replaced by the variable's value, as Windows
/// expands a stored Path. A `%` with no closing one, and a name with no
/// value, stay as written.
fn fill(value: &str, stored: Lookup, own: Lookup, depth: usize) -> String {
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
                    .then(|| stored(name))
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
fn folder_key(folder: &Path) -> String {
    path_key(folder.to_string_lossy().trim())
}

/// The file the host would start for `program` along `path`, as the command
/// prompt or a shell finds it; `None` when nothing is there.
///
/// A program with a folder in its name is started as named: whatever is
/// there is taken, even something that cannot start, which then fails to
/// start rather than reads as missing. On Windows a name without an
/// extension is tried with each extension in `PATHEXT` first, as Windows
/// would start it, and only then as written: an npm install puts a shell
/// script `claude` beside the `claude.cmd` Windows runs.
///
/// A bare name is looked for in each folder of `path` in turn, as the
/// platform names it ([`program_names`]): on Windows as given when it has
/// an extension, then with each extension in `PATHEXT`, since many
/// command-line tools, Claude Code's npm install among them, are `.cmd`
/// scripts rather than `.exe` files; on other systems a file with an
/// execute bit.
pub fn find_program(program: &str, path: Option<&str>) -> Option<PathBuf> {
    let as_given = Path::new(program);
    if as_given.components().count() != 1 {
        return program_names(program)
            .into_iter()
            .map(PathBuf::from)
            .chain(std::iter::once(as_given.to_path_buf()))
            .find(|candidate| candidate.exists());
    }
    let names = program_names(program);
    std::env::split_paths(path?)
        .filter(|folder| !folder.as_os_str().is_empty())
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
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

    fn joined(folders: &[&str]) -> String {
        std::env::join_paths(folders)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn nothing_stored(_: &str) -> Option<String> {
        None
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
            &[joined(&["/sys1", "/sys2"]), joined(&["/user1"])],
            &nothing_stored,
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
            &[joined(&["/b", "/c", "", "/a"]), joined(&["/c/", "/d"])],
            &nothing_stored,
            &own,
        );
        assert_eq!(
            composed.as_deref(),
            Some(joined(&["/a", "/b/", "/c", "/d"]).as_str())
        );
    }

    #[test]
    fn with_nothing_to_add_the_path_core_was_started_with_comes_back_as_given() {
        let inherited = joined(&["/a", "", "/b"]);
        let composed = compose(Some(&inherited), &[joined(&["/a"])], &nothing_stored, &own);
        assert_eq!(composed.as_deref(), Some(inherited.as_str()));
    }

    // verifies: PROTO-CORE-API-R148
    #[test]
    fn variables_are_filled_from_the_stored_values_then_cores_own_environment() {
        let stored = |name: &str| match name {
            "TOOLS" => Some("%HOME_DIR%/tools".to_string()),
            // The stored value wins over Core's own, which is older.
            "HOME_DIR" => Some("/stored/home".to_string()),
            _ => None,
        };
        let composed = compose(
            None,
            &[joined(&[
                "%HOME_DIR%/bin",
                "%TOOLS%/x",
                "%OWN_ONLY%/own",
                "%NOPE%/y",
                "50%off",
            ])],
            &stored,
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
        let stored = |name: &str| (name == "LOOP").then(|| "%LOOP%x".to_string());
        let composed = compose(None, &["%LOOP%".to_string()], &stored, &own).unwrap();
        assert!(composed.starts_with("%LOOP%"), "{composed}");
    }

    #[test]
    fn no_path_at_all_is_none() {
        assert_eq!(compose(None, &[], &nothing_stored, &own), None);
    }

    /// A full path is started as named: whatever is there is taken, even a
    /// folder, which cannot start; a full path to nothing is not found.
    // verifies: PROTO-CORE-API-R148
    #[test]
    fn a_full_path_is_taken_as_named_when_anything_is_there() {
        let folder = std::env::temp_dir();
        let named = folder.display().to_string();
        assert_eq!(find_program(&named, None), Some(folder.clone()));
        let nothing = folder.join("ersa-no-such-program-xyz");
        assert_eq!(find_program(&nothing.display().to_string(), None), None);
    }
}
