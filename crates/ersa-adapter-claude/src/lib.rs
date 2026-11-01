//! The Claude Code adapter. It drives the Claude Code CLI in its streaming JSON
//! mode, passing every behaviour-affecting flag explicitly, reads the backend's
//! capabilities from the CLI's initial message, and provides the projection from
//! the CLI's raw output lines to Ersa events. Permission requests reach Ersa
//! through the harness tools server rather than through this adapter (ADR 0012).
//!
//! The wire protocol it speaks is described in
//! `docs/briefs/PROTO-CLAUDE-CODE-STREAM.md`, with recorded fixtures under
//! `docs/briefs/protocol/fixtures/` for conformance tests. It starts the CLI
//! through Core's transport interface, never a concrete transport, so it runs
//! unchanged on local and remote hosts. There is no logic here yet.
