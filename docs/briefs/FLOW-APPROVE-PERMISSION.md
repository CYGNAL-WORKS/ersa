---
id: FLOW-APPROVE-PERMISSION
type: flow
title: Answer a session from the shelf
status: draft
references: [LAY-SHELL, PROTO-HARNESS-TOOLS]
adrs: [0012]
---

# Answer a session from the shelf

## Intent

An agent asks to run a command, edit a file, or use a tool, or asks the user a question. The session's tile in the rail marks that it needs the user. The user opens the tile's shelf, which shows what is being asked, where, and the session's context and cost so far, and answers it there: approve once, approve for the rest of the session, deny with an optional reason, or pick an answer to the question. The agent continues. The user never has to make the session active; several pending shelves across sessions are handled in turn from the rail. Opening the transcript is available for context but never required.

## Open questions

- How much of each backend's native permission vocabulary (allow once, allow always, deny, edit the command) is surfaced versus normalised.
- Whether an "approve all for this session" option exists and how it is made deliberately hard to hit by accident.
- Keyboard-only path: jump to the oldest pending shelf, answer, jump to the next.
- What the shelf shows for a permission whose details (diff, command) are large.

## Changes

- 2026-10-31: reframed around the shelf; the transcript is a drill-down, not the place to answer.
- 2026-10-31: created as draft.
