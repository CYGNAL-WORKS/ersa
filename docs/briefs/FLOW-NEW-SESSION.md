---
id: FLOW-NEW-SESSION
type: flow
title: Start a new session
status: draft
references: [LAY-SHELL]
adrs: []
---

# Start a new session

## Intent

From the rail, the user adds a project (a local folder or a folder on a known host), chooses or creates a workspace for it, picks a backend and a mode, and starts a session. The new session appears in the rail under its workspace and becomes active, with the composer focused. Creating a workspace creates an isolated checkout (a git worktree for git projects) on the project's host.

## Open questions

- Whether a session can be started without an explicit workspace (default workspace per project) to keep the common case to one click.
- How the backend list is populated: detection on the host, or user-declared.
- Whether mode is chosen at session start, inherited from the project, or both.

## Changes

- 2026-10-31: created as draft.
