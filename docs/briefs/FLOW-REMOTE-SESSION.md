---
id: FLOW-REMOTE-SESSION
type: flow
title: Run a session on a remote host
status: draft
references: [LAY-SHELL, FLOW-NEW-SESSION]
adrs: [0007]
---

# Run a session on a remote host

## Intent

The user adds a host from their SSH configuration (or a container), Ersa checks that it can connect and which backends are installed there, and the host appears as a place to add projects. From there the flow is `FLOW-NEW-SESSION`: the workspace is created on the remote host and the backend runs there. The session shows its host in the rail and transcript. If the connection drops, the session shows a disconnected state and reconnects when it can, without losing transcript.

## Open questions

- Container hosts: Docker exec against a named container, or a generic "command that gives me a shell" transport.
- How backend installation and authentication on the remote host are detected and explained when missing.
- Whether remote worktree roots are configurable per host.

## Changes

- 2026-10-31: created as draft.
