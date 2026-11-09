---
id: FLOW-RESUME-SESSION
type: flow
title: Resume sessions after restart
status: draft
references: [LAY-SHELL, PROTO-CORE-API]
adrs: [0011, 0017, 0031]
---

# Resume sessions after restart

## Intent

The user closes the window, or quits, or the machine restarts, and reopens Ersa. Every project, workspace, and session is where it was, apart from a session whose backend has no adapter in this build (PROTO-CORE-API-R66). Closing the window alone never stops a session, because Core runs on its own; reopening reattaches.

A session's history is the backend's own session record (ADR 0017), so its transcript after a quit or crash is whatever the backend recorded, including turns taken outside Ersa. Details only the live stream carries (text as it arrived, permission prompts, turn ends, cost) are not in it. Resuming starts the backend again with the session's backend id, and the session continues in the same record. A session that was mid-task shows it was interrupted and offers to continue.

Ersa lists, for a project, every session in the backend's record, including sessions started in the backend's own tools, and the user can resume any of them. Each session shows its backend id, so the user can resume it in the backend's own tools. Retention belongs to the backend: a session the backend has deleted no longer appears. Before resuming, Ersa checks the backend's record of live sessions and tells the user when the session is open in another process, since both would write to one record; it does not prevent it.

## Open questions

- Which directories a project's list covers: the project's own path, its workspaces' paths, or both, given that a backend records sessions by working directory.
- Where a session started elsewhere runs when resumed in Ersa: its own working directory, or a workspace Ersa chooses, and what Ersa does when that directory no longer exists.
- Remote hosts that are unreachable at launch.

## Changes

- 2026-11-09: a session whose backend has no adapter in this build is hidden until the adapter is back (ADR 0031).
- 2026-11-01: rewritten for ADR 0017: history is the backend's session record; sessions started elsewhere are listed and resumable; resume by backend id, shown so the user can resume elsewhere; retention is the backend's; live-elsewhere check. Open questions on resume semantics and on which record is replayed answered.
- 2026-10-31: window close no longer stops sessions; transcript from raw log.
- 2026-10-31: created as draft.
