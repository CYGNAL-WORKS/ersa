//! Ersa's Core: everything except the window. The Core process (the `ersa-core`
//! binary, built from the `ersa-cli` crate) is a thin wrapper around this library
//! (ADR 0011), and the same binary runs on remote hosts.
//!
//! Each module below is one part of Core as `ARCH-CORE` lists them. They are
//! empty for now; their doc comments state what each will own.
//!
//! Two rules hold for the whole crate. Nothing here depends on Tauri, so Core
//! builds and tests without a window or a display. Code that only works on one
//! operating system lives behind a narrow `platform` module (ADR 0006); there is
//! none yet. `tests/boundaries.rs` checks both.

// `pub mod name;` tells the compiler to load the module from `src/name.rs`.
pub mod adapters;
pub mod api;
pub mod harness_tools;
pub mod modes;
pub mod packs;
pub mod persistence;
pub mod raw_log;
pub mod sessions;
pub mod transport;
pub mod workspace;
