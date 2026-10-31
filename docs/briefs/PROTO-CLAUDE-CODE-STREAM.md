---
id: PROTO-CLAUDE-CODE-STREAM
type: protocol
title: Claude Code streaming JSON protocol
status: draft
references: [PROTO-AGENT-EVENTS]
adrs: [0004]
---

# Claude Code streaming JSON protocol

## Intent

The protocol Claude Code speaks when run non-interactively with streaming JSON output and streaming JSON input: the message types it emits (system, assistant, user, result, and control messages), the shape of each, how permission requests arrive on the control channel and how decisions are returned, how sessions are resumed, and how the process signals completion and errors. This brief is written by observing the real CLI, with recorded fixtures stored beside the schema, and names the minimum CLI version it describes. Where the vendor documents the protocol, this brief cites it; where it does not, this brief is the documentation.

## Open questions

- Exact message and control-request shapes in the current CLI version (to be recorded, not recalled).
- How system-prompt injection and settings are passed for modes.
- How MCP server configuration is supplied per session for the harness tools server.
- Behaviour on stdin close, SIGINT, and process kill, and what the last message is in each case.

## Changes

- 2026-10-31: created as draft.
