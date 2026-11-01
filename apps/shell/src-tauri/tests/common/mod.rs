//! Helpers shared by the Shell's integration tests. A `tests/common/`
//! module is Cargo's convention for code several test files include
//! (`mod common;`) without becoming a test binary of its own.

#![allow(dead_code)] // each test file uses only some of these

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// The `ersa-core` executable: `ERSA_CORE_BIN`, or the root workspace's
/// debug build (`cargo build -p ersa-cli` at the repository root).
pub fn core_program() -> PathBuf {
    let program = std::env::var_os("ERSA_CORE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            repo_root()
                .join("target/debug")
                .join(format!("ersa-core{}", std::env::consts::EXE_SUFFIX))
        });
    assert!(
        program.exists(),
        "no ersa-core at {}: run `cargo build -p ersa-cli` at the repository root, or set ERSA_CORE_BIN",
        program.display()
    );
    program
}

/// The repository root. CARGO_MANIFEST_DIR is apps/shell/src-tauri.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// An endpoint label no other test, and no Core of the user's, serves: the
/// process id and a counter.
pub fn unique_label(prefix: &str) -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
