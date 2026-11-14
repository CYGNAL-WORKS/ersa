//! Claude Code driven through Core: sessions, naming, detection, the
//! choices `backend.list` offers, and the replay server compared with a live
//! turn, all over Core's real socket with the real Claude adapter and a stub
//! CLI replaying recorded fixtures.
//!
//! These tests live with the adapter, not with Core, so Core builds and
//! passes its own tests with no adapter present (ARCH-CORE-R13). They are one
//! test target, so Core's socket harness is compiled once for all of them.

// Core's own test harness, shared by including its file: the in-process
// Core, its client, and the fake adapter. Its submodules resolve beside it.
#[path = "../../../ersa-core/tests/support/mod.rs"]
mod support;

// This crate's own test helpers, shared with its other test targets.
#[path = "../common/mod.rs"]
mod common;

mod choices;
mod claude;
mod detection;
mod naming;
mod replay;
mod session;
