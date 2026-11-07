---
id: FLOW-FORK-SESSION
type: flow
title: Fork a session
status: ready
references: [LAY-SHELL, PROTO-CORE-API, PROTO-CLAUDE-CODE-STREAM]
adrs: [0017]
---

# Fork a session

## Outcome

The user has a new session that carries another session's conversation up to its last finished reply and goes on from there on its own, while the original is unchanged.

## Trigger

The user chooses **Fork** in a tile's menu (`LAY-SHELL`, The rail).

## Steps

1. On `LAY-SHELL`, the user opens a session's tile menu. Fork is available when the session's conversation ends with a finished reply: one has finished, no reply is running, and the last turn was not interrupted.
2. The user chooses Fork. A new tile appears in the session's group and becomes the active session; its transcript holds the conversation up to that reply, and the composer is ready.
3. The fork runs in the same workspace as the original, with the original's backend, model, effort and permission mode, in the original's section, not pinned. Its title is what the backend gives it.
4. The original session keeps its state: a running original keeps running.

## Variations and errors

- **A reply is running**: Fork is unavailable, and says it is available once the reply finishes.
- **No reply has finished yet**: Fork is unavailable, and says it is available after the first reply.
- **The last turn was interrupted**, or Ersa stopped mid-turn: Fork is unavailable, and says it is available once a reply finishes.
- **The session is open in another process**, such as the backend's own terminal: Fork is unavailable, and says where it is open.
- **The backend can't fork** (the backend, or the version installed, has no way to): Fork is unavailable and says so.
- **The fork fails to start**: Ersa says so with the backend's reason, as a failed start does (FLOW-NEW-SESSION), and the original is unchanged.
- **A shared workspace**: the fork and the original share files, as two sessions in one workspace always do; deleting either keeps the workspace while the other uses it (FLOW-ARCHIVE-DELETE-SESSION).

## Requirements

- **FLOW-FORK-SESSION-R01**: When the user forks a session, Ersa shall start a new session whose conversation is the original's up to its last finished reply, under a new backend session id, leaving the original's record and state unchanged.
- **FLOW-FORK-SESSION-R02**: A fork shall run in the original's workspace, with the original's backend, model, effort, permission mode and section, and shall not be pinned or archived.
- **FLOW-FORK-SESSION-R03**: Unless the session's conversation, as its record holds it, ends with a finished reply and no process has the session open elsewhere, Ersa shall offer Fork as unavailable with the reason, and Core shall refuse `session.fork` with `invalid_state`.
- **FLOW-FORK-SESSION-R04**: When a fork has started, Ersa shall make it the active session.

## Out of scope

- Forking from an earlier reply than the last finished one.
- Forking into a new isolated workspace.

## Open questions

- What a fork is named. The owner expects it to be named as Claude Code names one, likely the original's name marked as a fork; the CLI's fork is not yet recorded, so this is decided when forking is built.

## Changes

- 2026-11-07: open question on how a fork is named. No requirement IDs changed.
- 2026-11-05: created from the owner's decisions of 2026-11-05: Fork starts a new session from the last finished reply, unavailable with the reason while a reply runs or before one has finished. R01 to R04 added.
