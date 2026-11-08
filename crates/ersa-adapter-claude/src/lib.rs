//! The Claude Code adapter. It drives the Claude Code CLI in its streaming JSON
//! mode, passing every behaviour-affecting flag explicitly, and provides the
//! projection from the raw log of that conversation to Ersa events. Permission
//! requests arrive over the CLI's stdio control channel as `can_use_tool`
//! requests, which this adapter answers (ADR 0014).
//!
//! The wire protocol it speaks is described in
//! `docs/briefs/PROTO-CLAUDE-CODE-STREAM.md`, with recorded fixtures under
//! `docs/briefs/protocol/fixtures/` for conformance tests. It starts the CLI
//! through Core's transport interface, never a concrete transport, so it runs
//! unchanged on local and remote hosts.
//!
//! - [`ClaudeAdapter`] starts and drives one CLI process per session.
//! - [`ClaudeProjector`] turns that session's raw log into Ersa events.
//! - [`record`] reads the CLI's own session record, which is a session's
//!   history (ADR 0017): its projection, the list of a directory's sessions,
//!   and the registry of sessions open in running CLI processes.
//! - [`wire`] holds the typed models of every line in both directions.

mod attachments;
mod driver;
mod platform;
mod projector;
pub mod record;
mod suggestions;
pub mod wire;

pub use driver::{ClaudeAdapter, invocation_args, resume_args, session_id_args};
pub use projector::ClaudeProjector;

/// The backend's name in `source.backend`, `session.started`, and the Core API.
pub const BACKEND: &str = "claude-code";

/// The oldest CLI version the invocation was verified on: the setting
/// sources per trust, the entrypoint and the environment
/// (`PROTO-CLAUDE-CODE-STREAM`, Versioning). Older fixtures still parse.
pub const MIN_TESTED_VERSION: &str = "2.1.316";

/// The newest CLI version the fixtures were recorded on. A newer CLI still
/// runs, with a `session.notice` saying so (PROTO-CLAUDE-CODE-STREAM-R10).
pub const MAX_TESTED_VERSION: &str = "2.1.316";
