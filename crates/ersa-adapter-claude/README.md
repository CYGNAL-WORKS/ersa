# ersa-adapter-claude

The adapter that drives the Claude Code CLI in its streaming JSON mode and projects its raw output into Ersa events. The protocol it speaks is described in `docs/briefs/PROTO-CLAUDE-CODE-STREAM.md`, with recorded fixtures for its conformance tests. Permission prompts arrive over the CLI's stdio control channel and are answered there (ADR 0014). It reaches the CLI only through Core's transport interface, never a concrete local or SSH transport, so it runs unchanged on any host. It must never depend on Tauri, parse terminal output, or run the CLI in a pseudo-terminal.

It has three parts:

- `wire`: typed serde models of every message the CLI writes and accepts, with unknown kinds preserved as raw JSON. `tests/fixture_conformance.rs` parses every recorded fixture line through them.
- The driver (`ClaudeAdapter`): starts the CLI with explicit flags, performs the `initialize` handshake, records every line in both directions to Core's sink, and answers `can_use_tool` prompts. `tests/driver.rs` runs it against stub CLIs, including a byte-for-byte replay of fixture 11's permission round trip.
- The projection (`ClaudeProjector`): turns a session's raw log into Ersa events. `tests/projection.rs` projects every fixture and compares the result with `tests/golden/`; after an intended change, rewrite the goldens with `ERSA_BLESS=1 cargo test -p ersa-adapter-claude --test projection` and review the diff.

`tests/live.rs` drives the installed CLI through one short prompt with a permission round trip. It is ignored by default because it spends a model call on the machine's Claude login; run it by hand with `cargo test -p ersa-adapter-claude --test live -- --ignored --nocapture`.
