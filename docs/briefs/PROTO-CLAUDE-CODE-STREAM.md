---
id: PROTO-CLAUDE-CODE-STREAM
type: protocol
title: Claude Code streaming JSON protocol
status: draft
references: [PROTO-AGENT-EVENTS, PROTO-HARNESS-TOOLS]
adrs: [0004, 0012]
---

# Claude Code streaming JSON protocol

## Intent

The protocol Claude Code speaks in its streaming JSON mode (print mode with stream-json input and output, run as a long-lived bidirectional session): the exact command line Ersa uses with every behaviour-affecting flag passed explicitly; the message types emitted (system, assistant, user, result, and any control messages) and their shapes; the capabilities announced in the initial system message and how the adapter feature-detects from them; how permission requests reach Ersa through the permission-prompt tool on the harness tools server and how decisions return; how sessions are identified and resumed; how usage is reported; and how the process signals completion and errors on stdin close, interrupt, and kill. Written by observing the real CLI, with recorded fixtures stored beside the schema, naming the minimum and maximum tested CLI versions. Where the vendor documents the protocol this brief cites it; where it does not, this brief is the documentation.

## Open questions

- Exact message shapes in the current CLI version (to be recorded, not recalled).
- Whether the permission-prompt tool receives enough context (command, paths, diff) to render the shelf, or whether the adapter must correlate with the preceding tool-use message.
- How system-prompt additions and settings are passed for modes, and how MCP server configuration for the harness tools server is supplied per session.
- Whether Ersa replays its own raw log or the CLI's own session file on resume.

## Changes

- 2026-10-31: explicit flags, feature detection, and permission routing via the harness tools server per ADR 0012.
- 2026-10-31: created as draft.
