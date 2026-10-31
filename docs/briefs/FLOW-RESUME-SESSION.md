---
id: FLOW-RESUME-SESSION
type: flow
title: Resume sessions after restart
status: draft
references: [LAY-SHELL]
adrs: []
---

# Resume sessions after restart

## Intent

The user closes Ersa, or the machine restarts, and reopens it. Every project, workspace, and session is where it was. Each session's transcript is complete. A session that was mid-task shows that it was interrupted and offers to continue; continuing hands the backend its own resumable session state where the backend supports it, and otherwise starts a fresh backend process with the transcript preserved as history.

## Open questions

- Each backend's resume semantics and what Ersa guarantees when the backend cannot resume.
- Whether sessions auto-reconnect on launch or wait for the user to open them.
- Remote hosts that are unreachable at launch.

## Changes

- 2026-10-31: created as draft.
