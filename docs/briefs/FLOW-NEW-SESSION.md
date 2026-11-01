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

## Built in M0

The least that reaches a session: a project is added by typing its folder path; "New session" on a git project creates a new worktree workspace and a Claude Code session in it, every time; a non-git project offers no session; sessions are a flat list in the rail. With no session chosen, the window shows the newest, so a reopened window shows the one that was running.

## Open questions

- Whether a session can be started without an explicit workspace (default workspace per project) to keep the common case to one click.
- How the backend list is populated: detection on the host, or user-declared.
- Whether mode is chosen at session start, inherited from the project, or both.
- When a new session reuses an existing workspace rather than creating one, and what happens to a workspace whose session failed to start.
- Which session a reopened window shows when several were running.

## Changes

- 2026-11-01: from #14 inbox: what M0 built; workspace reuse and the reopened window's session added as open questions.
- 2026-10-31: created as draft.
