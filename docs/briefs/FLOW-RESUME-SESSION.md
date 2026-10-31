---
id: FLOW-RESUME-SESSION
type: flow
title: Resume sessions after restart
status: draft
references: [LAY-SHELL]
adrs: [0010, 0011]
---

# Resume sessions after restart

## Intent

The user closes the window, or quits, or the machine restarts, and reopens Ersa. Every project, workspace, and session is where it was. Closing the window alone never stops a session, because Core runs on its own; reopening reattaches. After a quit or crash, each session's transcript is complete because it is projected from the raw log. A session that was mid-task shows it was interrupted and offers to continue; continuing starts the backend again with its own resumable session state where the backend supports it, and otherwise starts fresh with the transcript preserved as history.

## Open questions

- Each backend's resume semantics and what Ersa guarantees when the backend cannot resume.
- Whether Ersa replays its raw log or the backend's own session record on resume, and what happens when they disagree.
- Remote hosts that are unreachable at launch.

## Changes

- 2026-10-31: window close no longer stops sessions; transcript from raw log.
- 2026-10-31: created as draft.
