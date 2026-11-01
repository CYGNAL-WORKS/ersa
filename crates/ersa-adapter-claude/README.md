# ersa-adapter-claude

The adapter that drives the Claude Code CLI in its streaming JSON mode and projects its raw output into Ersa events. The protocol it speaks is described in `docs/briefs/PROTO-CLAUDE-CODE-STREAM.md`, with recorded fixtures for its conformance tests. It reaches the CLI only through Core's transport interface, never a concrete local or SSH transport, so it runs unchanged on any host. It must never depend on Tauri, parse terminal output, or run the CLI in a pseudo-terminal. There is no logic here yet.
