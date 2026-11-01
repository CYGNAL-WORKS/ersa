# ersa-adapter-claude

The adapter that drives the Claude Code CLI in its streaming JSON mode and projects its raw output into Ersa events. The protocol it speaks is described in `docs/briefs/PROTO-CLAUDE-CODE-STREAM.md`, with recorded fixtures for its conformance tests. Permission prompts arrive over the CLI's stdio control channel and are answered there (ADR 0014). It reaches the CLI only through Core's transport interface, never a concrete local or SSH transport, so it runs unchanged on any host. It must never depend on Tauri, parse terminal output, or run the CLI in a pseudo-terminal.

So far it holds the `wire` module: typed serde models of every message the CLI writes and accepts, with unknown kinds preserved as raw JSON. `tests/fixture_conformance.rs` parses every recorded fixture line through them. The driver and projection come later.
