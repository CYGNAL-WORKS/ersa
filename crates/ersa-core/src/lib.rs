//! Ersa's Core: everything except the window. The Core process (the `ersa-core`
//! binary, built from the `ersa-cli` crate) is a thin wrapper around this library
//! (ADR 0011), and the same binary runs on remote hosts.
//!
//! Most modules below are one part of Core as `ARCH-CORE` lists them; the
//! ones still empty state in their doc comments what each will own. Three
//! more hold what those parts share: `state` (the [`Core`] every request
//! handler receives), `settings`, and `platform`.
//!
//! [`Server::bind`] then [`Server::run`] is the whole Core process: claim the
//! socket, open the database, answer requests until `core.shutdown`.
//!
//! Two rules hold for the whole crate. Nothing here depends on Tauri, so Core
//! builds and tests without a window or a display. Code that only works on one
//! operating system lives behind the narrow `platform` module (ADR 0006).
//! `tests/boundaries.rs` checks both.

// `pub mod name;` tells the compiler to load the module from `src/name.rs`.
pub mod adapters;
pub mod api;
pub mod harness_tools;
pub mod modes;
pub mod packs;
pub mod persistence;
pub mod platform;
pub mod raw_log;
pub mod replay;
pub mod sessions;
pub mod settings;
pub mod state;
pub mod transport;
pub mod workspace;

pub use api::server::{ServeError, Server, connect};
// What a binary needs to register adapters with `Server::bind_with`.
pub use ersa_adapter::Adapter;
pub use state::{Core, CoreConfig};
