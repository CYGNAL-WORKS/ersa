# ersa-adapter-claude

The adapter that drives the Claude Code CLI in its streaming JSON mode and projects its raw output into Ersa events. The protocol it speaks is described in `docs/briefs/PROTO-CLAUDE-CODE-STREAM.md`, with recorded fixtures for its conformance tests. Permission prompts arrive over the CLI's stdio control channel and are answered there (ADR 0014). It reaches the CLI only through Core's transport interface, never a concrete local or SSH transport, so it runs unchanged on any host. It must never depend on Tauri, parse terminal output, or run the CLI in a pseudo-terminal.

It has four parts:

- `wire`: typed serde models of every message the CLI writes and accepts, with unknown kinds preserved as raw JSON. `tests/fixture_conformance.rs` parses every recorded fixture line through them.
- The driver (`ClaudeAdapter`): starts the CLI with explicit flags, performs the `initialize` handshake, records every line in both directions to Core's sink, and answers `can_use_tool` prompts. `tests/driver.rs` runs it against stub CLIs, including a byte-for-byte replay of fixture 11's permission round trip.
- The projection (`ClaudeProjector`): turns a live session's raw lines into Ersa events, tagging each message's events with the id the session record shares. `tests/projection.rs` projects every fixture and compares the result with `tests/golden/`; after an intended change, rewrite the goldens with `ERSA_BLESS=1 cargo test -p ersa-adapter-claude --test projection` and review the diff.

- `record`: the CLI's own session record, which is a session's history (ADR 0017). `project_record` projects the conversation's branch of a record through the same message projection as the live stream; `list_sessions` lists a directory's sessions from the head and tail of each record; `live_session` reads the registry of sessions open in running CLI processes. `tests/session_record.rs` checks each record fixture against the stream recorded for the same run and against `tests/golden/<version>/record/`; `tests/resume.rs` covers resume and the registry. Record a new fixture pair with `node scripts/record-claude-session.mjs`.

`tests/through_core/` drives Claude Code through Core's API over its real socket: sessions, naming, detection, `backend.list`'s choices, and a replay compared with a live turn. It includes Core's socket test harness from `crates/ersa-core/tests/support/`, so Core itself builds and tests with no adapter.

`tests/live.rs` drives the installed CLI through one short prompt with a permission round trip. It is ignored by default because it spends a model call on the machine's Claude login; run it by hand with `cargo test -p ersa-adapter-claude --test live -- --ignored --nocapture`.
