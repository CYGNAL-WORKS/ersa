---
id: PROTO-HARNESS-TOOLS
type: protocol
title: Harness tools server
status: draft
references: [PROTO-AGENT-EVENTS]
adrs: []
---

# Harness tools server

## Intent

An MCP server that Ersa exposes to every session's backend. It offers tools the agent can call to interact with Ersa itself: list sessions and their states, send a message to another session, read session metadata, and the "show me" tools for driving the interface (open a pane, highlight a control, fill a field, explain). Access is per session with a token, so one session cannot impersonate another. Interface-driving tools require the user's consent for the session and never perform irreversible actions.

## Open questions

- Tool list for the first milestone that needs it (M6), and whether cross-session messaging ships earlier.
- How the Interface exposes its controls to the server: a static identifier map or a live tree.
- Rate limits and whether tool calls appear in the calling session's transcript.

## Changes

- 2026-10-31: created as draft.
