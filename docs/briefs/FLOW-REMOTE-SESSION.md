---
id: FLOW-REMOTE-SESSION
type: flow
title: Run a session on a remote host
status: draft
references: [LAY-SHELL, FLOW-NEW-SESSION]
adrs: [0007, 0011, 0017]
---

# Run a session on a remote host

## Intent

The user adds a host from their SSH configuration. Ersa connects, checks which backends and version control tools are installed there, and places or updates its own Core on the host. The host appears as a place to add projects. From there the flow is `FLOW-NEW-SESSION`: the workspace is created on the remote host and the backend runs there, under the remote Core. The session shows its host in its tile and transcript. If the connection drops, the session shows a disconnected state; on reconnect Ersa resumes the backend session and shows anything emitted during the outage from the backend's session record on the remote host. A turn that was in flight when the connection dropped is reported as interrupted, not silently lost.

## Open questions

- Containers: a host that is a container exec, or a container reached over SSH.
- How the remote Core is installed and updated, and what happens when its version differs from the local one.
- How missing backend installation or authentication on the remote host is detected and explained.
- Whether remote workspace roots are configurable per host.
- What the rail shows for a remote host's sessions when the host can't be reached as Ersa starts (from FLOW-RESUME-SESSION).

## Changes

- 2026-11-13: open question on unreachable hosts at start, moved from FLOW-RESUME-SESSION. No requirement IDs changed.
- 2026-11-01: backfill after a disconnect comes from the backend's session record on the remote host (ADR 0017).
- 2026-10-31: remote Core, reconnect defined as resume plus backfill, interrupted turns surfaced.
- 2026-10-31: created as draft.
