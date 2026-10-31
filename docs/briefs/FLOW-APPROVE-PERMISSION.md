---
id: FLOW-APPROVE-PERMISSION
type: flow
title: Approve or deny a permission request
status: draft
references: [LAY-SHELL]
adrs: []
---

# Approve or deny a permission request

## Intent

An agent asks to run a command, edit a file, or use a tool. The session's rail entry shows it is waiting. The user opens the session and sees a permission tile in the transcript with what is being asked, where, and the session's cost and context so far. The user approves once, approves for the rest of the session, or denies with an optional reason, and the agent continues. Multiple pending requests across sessions can be handled in turn from the rail without losing place.

## Open questions

- How much of each backend's native permission vocabulary (allow once, allow always, deny, edit the command) is surfaced versus normalised.
- Whether an "approve all for this session" option exists and how it is made deliberately hard to hit by accident.
- Keyboard-only approval flow from the rail.

## Changes

- 2026-10-31: created as draft.
