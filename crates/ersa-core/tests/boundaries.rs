//! Checks that Core keeps the architectural boundaries `ARCH-CORE` sets.
//!
//! Files in a crate's `tests/` directory are integration tests: Cargo compiles
//! each one as its own small crate that runs under `cargo test`. These tests read
//! files from the workspace rather than calling Core's code, because what they
//! check is the shape of the build, not behaviour.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The workspace root. `CARGO_MANIFEST_DIR` is set by Cargo at compile time to
/// this crate's directory (`crates/ersa-core`), so the root is two levels up.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/ersa-core sits two levels below the workspace root")
        .to_path_buf()
}

/// The workspace as `cargo metadata` describes it: every package with the
/// dependencies its manifest declares, and the resolved graph. `--all-features`
/// adds every optional dependency, so nothing behind a feature that is off by
/// default escapes a check; `--locked` reads the lockfile as it is and never
/// changes it. Without `--filter-platform` the graph covers every platform.
fn cargo_metadata() -> Value {
    // CARGO is the cargo that is building this test, so the check uses the
    // same toolchain as the build it checks.
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--all-features",
            "--locked",
        ])
        .current_dir(workspace_root())
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata prints JSON")
}

/// Every package a plain `cargo build` or `cargo test` at the root can compile:
/// the workspace's default members (Core's crates) and everything they depend
/// on, directly or transitively, including dev and build dependencies and
/// dependencies for every target OS.
///
/// The Shell is a workspace member too, so `Cargo.lock` lists Tauri; what
/// matters is whether Core's crates reach it. `cargo metadata` resolves the
/// graph: `workspace_default_members` names where to start, and each node of
/// `resolve` lists the package ids it depends on, on every platform and with
/// every feature on, so the check holds for all of them.
fn default_members_dependency_closure() -> BTreeSet<String> {
    let metadata = cargo_metadata();

    // Package ids are opaque strings; the names are on `packages`.
    let name_of: BTreeMap<&str, &str> = metadata["packages"]
        .as_array()
        .expect("metadata has packages")
        .iter()
        .map(|p| (p["id"].as_str().unwrap(), p["name"].as_str().unwrap()))
        .collect();
    let deps_of: BTreeMap<&str, Vec<&str>> = metadata["resolve"]["nodes"]
        .as_array()
        .expect("metadata has a resolved graph")
        .iter()
        .map(|node| {
            let deps = node["deps"]
                .as_array()
                .expect("every node lists its dependencies")
                .iter()
                .map(|d| d["pkg"].as_str().unwrap())
                .collect();
            (node["id"].as_str().unwrap(), deps)
        })
        .collect();

    let mut seen = BTreeSet::new();
    let mut queue: VecDeque<&str> = metadata["workspace_default_members"]
        .as_array()
        .expect("metadata names the default members (cargo 1.71 or later)")
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    while let Some(id) = queue.pop_front() {
        if seen.insert(id) {
            queue.extend(deps_of.get(id).into_iter().flatten());
        }
    }
    seen.into_iter().map(|id| name_of[id].to_string()).collect()
}

// verifies: ARCH-CORE-R01
#[test]
fn no_default_member_depends_on_tauri() {
    let deps = default_members_dependency_closure();
    assert!(
        deps.contains("ersa-core"),
        "the walk should start from Core's crates; found {deps:?}"
    );

    // Tauri's own crates, and the window and webview crates it draws with,
    // which need a display just as much.
    let display_stack = [
        "wry",
        "tao",
        "webview2-com",
        "gtk",
        "gdk",
        "soup3",
        "webkit2gtk",
        "javascriptcore-rs",
    ];
    let tauri: Vec<_> = deps
        .iter()
        .filter(|d| d.starts_with("tauri") || display_stack.contains(&d.as_str()))
        .collect();
    assert!(
        tauri.is_empty(),
        "Core must build without Tauri, but the default members depend on {tauri:?}"
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

/// The binary, `ersa-cli`: the one package that registers adapters (ADR 0031).
const REGISTERING_PACKAGE: &str = "ersa-cli";

/// A concrete adapter is a workspace crate named `ersa-adapter-<backend>`;
/// `ersa-adapter` itself is the boundary every adapter and Core share.
const ADAPTER_PREFIX: &str = "ersa-adapter-";

/// The workspace's members, by name, with the folder each one's manifest is in.
fn workspace_members(metadata: &Value) -> Vec<(String, PathBuf, &Value)> {
    let members: BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .expect("metadata names the workspace members")
        .iter()
        .map(|id| id.as_str().unwrap())
        .collect();
    metadata["packages"]
        .as_array()
        .expect("metadata has packages")
        .iter()
        .filter(|p| members.contains(p["id"].as_str().unwrap()))
        .map(|p| {
            let manifest = PathBuf::from(p["manifest_path"].as_str().unwrap());
            let dir = manifest.parent().unwrap().to_path_buf();
            (p["name"].as_str().unwrap().to_string(), dir, p)
        })
        .collect()
}

// verifies: ARCH-CORE-R13
#[test]
fn only_the_binary_depends_on_a_concrete_adapter() {
    let metadata = cargo_metadata();
    let members = workspace_members(&metadata);
    assert!(
        members.iter().any(|(name, _, _)| name == "ersa-core"),
        "the workspace should include Core"
    );

    // Every dependency each member's manifest declares, of every kind
    // (normal, dev, build) and on every platform, so a test-only dependency
    // of Core on an adapter is caught as surely as a normal one.
    let mut offenders = Vec::new();
    for (name, _, package) in &members {
        if name == REGISTERING_PACKAGE {
            continue;
        }
        for dep in package["dependencies"].as_array().unwrap() {
            let dep_name = dep["name"].as_str().unwrap();
            if dep_name.starts_with(ADAPTER_PREFIX) && dep_name != name {
                let kind = dep["kind"].as_str().unwrap_or("normal");
                offenders.push(format!("{name} -> {dep_name} ({kind})"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "only {REGISTERING_PACKAGE} may depend on a concrete adapter, so Core          builds and tests with none present:
{}",
        offenders.join("
")
    );
}

/// Every Rust source and Cargo manifest under `dir`, skipping build output.
fn sources_and_manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("readable directory") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            if !matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some("target" | "gen" | "node_modules")
            ) {
                sources_and_manifests(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            || path.file_name().is_some_and(|n| n == "Cargo.toml")
        {
            out.push(path);
        }
    }
}

// verifies: ARCH-CORE-R13
#[test]
fn no_crate_names_an_adapter_outside_its_registration() {
    let metadata = cargo_metadata();
    let members = workspace_members(&metadata);
    // A crate name, and the name Rust code uses for it.
    let adapters: Vec<(String, String)> = members
        .iter()
        .filter(|(name, _, _)| name.starts_with(ADAPTER_PREFIX))
        .map(|(name, _, _)| (name.clone(), name.replace('-', "_")))
        .collect();

    // Adding or removing a backend may touch its own crate and the binary's
    // registration: its `main.rs` and the feature in its manifest.
    let registration: Vec<PathBuf> = members
        .iter()
        .filter(|(name, _, _)| name == REGISTERING_PACKAGE)
        .flat_map(|(_, dir, _)| [dir.join("src").join("main.rs"), dir.join("Cargo.toml")])
        .collect();
    assert_eq!(
        registration.len(),
        2,
        "the workspace should include the binary"
    );

    let mut offenders = Vec::new();
    for (name, dir, _) in &members {
        let mut files = Vec::new();
        sources_and_manifests(dir, &mut files);
        for file in files.iter().filter(|f| !registration.contains(f)) {
            let text = fs::read_to_string(file).expect("readable file");
            for (crate_name, rust_name) in &adapters {
                if crate_name != name && (text.contains(crate_name) || text.contains(rust_name)) {
                    offenders.push(format!("{}: {crate_name}", file.display()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "an adapter is named outside its own crate and its registration:
{}",
        offenders.join(
            "
"
        )
    );
}
