//! Checks that Core keeps the architectural boundaries `ARCH-CORE` sets.
//!
//! Files in a crate's `tests/` directory are integration tests: Cargo compiles
//! each one as its own small crate that runs under `cargo test`. These tests read
//! files from the workspace rather than calling Core's code, because what they
//! check is the shape of the build, not behaviour.

use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// The workspace root. `CARGO_MANIFEST_DIR` is set by Cargo at compile time to
/// this crate's directory (`crates/ersa-core`), so the root is two levels up.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/ersa-core sits two levels below the workspace root")
        .to_path_buf()
}

/// Every package any workspace crate depends on, directly or transitively,
/// including dev and build dependencies and dependencies for every target OS.
///
/// Read from `Cargo.lock`, which lists every package in the build with the names
/// of its dependencies. Workspace crates are the entries without a `source`
/// (they come from this repository, not a registry). From those we walk the
/// dependency lists breadth-first. A dependency entry is `"name"` or
/// `"name version"` or `"name version (source)"`; the name is the first word.
/// When two versions of one package exist we follow both, which can only make
/// the set larger, so the check stays safe.
fn all_dependencies_of_workspace() -> BTreeSet<String> {
    let lock_path = workspace_root().join("Cargo.lock");
    let text = fs::read_to_string(&lock_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", lock_path.display()));
    let lock: toml::Table = text.parse().expect("Cargo.lock is valid TOML");

    let packages = lock["package"].as_array().expect("Cargo.lock has packages");
    let name_of = |p: &toml::Value| p["name"].as_str().unwrap_or_default().to_string();
    let deps_of = |name: &str| -> Vec<String> {
        packages
            .iter()
            .filter(|p| name_of(p) == name)
            .filter_map(|p| p.get("dependencies").and_then(|d| d.as_array()))
            .flatten()
            .filter_map(|d| d.as_str())
            .filter_map(|d| d.split_whitespace().next())
            .map(str::to_string)
            .collect()
    };

    let mut seen = BTreeSet::new();
    let mut queue: VecDeque<String> = packages
        .iter()
        .filter(|p| p.get("source").is_none())
        .map(name_of)
        .collect();
    while let Some(name) = queue.pop_front() {
        if seen.insert(name.clone()) {
            queue.extend(deps_of(&name));
        }
    }
    seen
}

// verifies: ARCH-CORE-R01
#[test]
fn no_workspace_crate_depends_on_tauri() {
    let deps = all_dependencies_of_workspace();
    assert!(
        deps.contains("ersa-core"),
        "the walk should start from the workspace crates; found {deps:?}"
    );

    let tauri: Vec<_> = deps.iter().filter(|d| d.starts_with("tauri")).collect();
    assert!(
        tauri.is_empty(),
        "Core must build without Tauri, but the workspace depends on {tauri:?}"
    );
}

/// Text that only appears in code using a Windows-only API or compiling only on Windows.
const WINDOWS_ONLY_MARKERS: &[&str] = &[
    "cfg(windows)",
    "target_os = \"windows\"",
    "target_family = \"windows\"",
    "windows_sys::",
    "windows::",
    "winapi::",
    "std::os::windows",
];

/// All `.rs` files under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// A file is inside a platform module when any part of its path is `platform`
/// (a `platform/` directory) or the file itself is `platform.rs`.
fn is_platform_module(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str() == "platform" || c.as_os_str() == "platform.rs")
}

// verifies: ARCH-CORE-R07
#[test]
fn windows_only_code_lives_in_platform_modules() {
    let crates_dir = workspace_root().join("crates");
    let mut files = Vec::new();
    for entry in fs::read_dir(&crates_dir).expect("crates directory exists") {
        let src = entry.expect("readable entry").path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    assert!(
        !files.is_empty(),
        "found no source files under {crates_dir:?}"
    );

    let mut offenders = Vec::new();
    for file in files.iter().filter(|f| !is_platform_module(f)) {
        let text = fs::read_to_string(file).expect("readable source file");
        for marker in WINDOWS_ONLY_MARKERS {
            if text.contains(marker) {
                offenders.push(format!("{}: {marker}", file.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "Windows-only code outside a platform module:\n{}",
        offenders.join("\n")
    );
}
