---
id: FLOW-RESUME-SESSION
type: flow
title: Resume sessions after restart
status: ready
references: [LAY-SHELL, PROTO-CORE-API, PROTO-AGENT-EVENTS, FLOW-ARCHIVE-DELETE-SESSION]
adrs: [0011, 0031, 0033]
---

# Resume sessions after restart

## Outcome

After the window closes, Ersa quits or the machine restarts, every project, workspace and session is where it was, and any session the backend has a record of, started in Ersa or in the backend's own tools, continues in the same record when the user writes to it.

## Trigger

The user reopens Ersa, opens a session that is not running, and writes to it; or chooses Resume on an archived session.

## Steps

1. On `LAY-SHELL`, the rail lists every session it listed before, apart from a dormant one (PROTO-CORE-API-R66). Closing the window alone never stopped a session, because Core runs on its own; reopening reattaches to those still running.
2. A session that is not running shows its history from the backend's own session record (ADR 0033): everything the record keeps, turns taken outside Ersa included. Details only the live stream carries (text as it arrived, permission prompts, cost) are not in it. A session that was live when Core stopped says so.
3. The user writes to it and sends. Ersa starts the backend again with the session's backend id, in the session's folder, and the message goes as the first turn of the resumed session, in the same record. The composer says the backend is starting meanwhile (`FLOW-SEND-MESSAGE`).
4. On an archived session, Resume on the archived note unarchives the session and resumes it, with no message.
5. A project's sessions include those the backend recorded in the project's folder or in any of its workspaces' folders, started in the backend's own tools included. They sit in the rail like any other, and resume the same way, in the folder they ran in.
6. From a tile's menu, Copy session ID copies the backend's id for the session, so the user can resume it in the backend's own tools.

## Variations and errors

- **The session's folder is gone** (a workspace removed, a folder deleted or moved): Ersa refuses to resume it and says so, in the composer's status line or above the archived note, and the session keeps its history and stays where it is.
- **The session is open in another process** (the backend's own tools): before starting the backend, Ersa says so in the transcript (PROTO-AGENT-EVENTS-R14) and resumes anyway, since the user may mean it; both would write to one record.
- **The backend's record is gone** (its retention removed it): a session started in the backend's own tools leaves the list the next time Ersa lists sessions. A session Ersa started stays, since a new session or a fork has no record until its first message.
- **Removing a workspace or project** with sessions started in the backend's own tools: Ersa forgets those sessions and leaves their records, so they show again in the backend's own tools and if the folder is added again.
- **The backend fails to start**: the message stays in the composer with the reason, as a refused message does (`FLOW-SEND-MESSAGE`), and the session stays stopped.
- **Stop while a message waits for the session to start** cancels the message and leaves the session resumed and idle (PROTO-CORE-API-R86).
- **A fork closed before its first message** has no record of its own, so resuming it forks its original again, under the conditions a fork has (R10). The agent's conversation in the fork then includes what the original did since, though Ersa shows the fork up to its fork point until its first message gives it a record.
- **A message sent while the session is being archived** is refused, not resumed (PROTO-CORE-API-R85); the user sends again.
- **A session whose backend has no adapter in this build** is dormant until the adapter is back (ADR 0031).

## Requirements

- **FLOW-RESUME-SESSION-R01**: When `session.resume` names a session that is not running, Core shall start its backend again with the session's backend id, in the session's folder, so that the session continues in the same record and its transcript keeps everything before; a session whose backend has no record of it yet (a new session or a fork before its first message) starts again as `PROTO-CLAUDE-CODE-STREAM` (Invocation) says, and a session already running is left as it is.
- **FLOW-RESUME-SESSION-R02**: When the user sends a message to a session that is not running and not archived, Core shall resume the session and then send the message as its first turn, reporting the backend starting as the send's phase.
- **FLOW-RESUME-SESSION-R03**: When a session's folder no longer exists, Core shall refuse to resume it, or to send to it while it is not running, with `invalid_state` and reason `folder_gone`, leaving the session and its arrangement unchanged; Core checks the folder itself before starting the backend.
- **FLOW-RESUME-SESSION-R04**: `session.list` shall include, for each project, every session the backend's record holds for the project's folder and for each of its workspaces' folders, those started in the backend's own tools included, each in the workspace whose folder it ran in, counting only records whose own working folder is that folder.
- **FLOW-RESUME-SESSION-R05**: A session started in the backend's own tools shall keep the same Ersa id across `session.list` calls and Core restarts, so that its arrangement (pinned, section, archived, unseen) is kept.
- **FLOW-RESUME-SESSION-R06**: When the backend's record of a session started in the backend's own tools is gone, `session.list` shall leave the session out from then on.
- **FLOW-RESUME-SESSION-R07**: `session.list` shall carry each session's `backend_session_id` once the backend has reported it, and the tile's menu shall offer Copy session ID, which puts it on the clipboard.
- **FLOW-RESUME-SESSION-R08**: When Core starts and finds a session that was live when it last stopped, the session's transcript shall end with its `session.ended` (PROTO-AGENT-EVENTS, Core's lifecycle facts), and the session shall be stopped and resumable.
- **FLOW-RESUME-SESSION-R09**: Sessions started in the backend's own tools shall not hold back removing their workspace or project: Core shall forget them, leaving their records.
- **FLOW-RESUME-SESSION-R10**: When a resume, by `session.resume` or by a send, names a fork that has no record of its own, Core shall fork the fork's original again as `session.fork` does, refusing the resume with `session.fork`'s answer whenever `session.fork` of the original would be refused and holding sends to the original until the fork's backend is ready.

## Built so far

Core resumes a stopped session on `session.resume` and on a send to it (R01, R02), refuses when its folder is gone (R03), and ends the transcript of a session that was live when Core last stopped (R08). A resumed session starts with the model, effort and permission mode Core last gave it; the record's newer values (PROTO-CORE-API-R71) are still to build, as are a recordless fork's conditions (R10) and the sessions started outside Ersa (R04 to R07, R09).

## Out of scope

- Remote hosts that can't be reached when Ersa starts (`FLOW-REMOTE-SESSION`, M4).
- A Continue button on an interrupted session: writing "continue" resumes it, as in Claude Desktop.
- Moving a session to another folder or workspace before resuming it.

## Open questions

- Whether a transcript that is open when its session resumes shows turns taken in the backend's own tools since it was opened; today they show when the session is next opened (PROTO-AGENT-EVENTS, Open questions).

## Changes

- 2026-11-13: from #353 inbox: Stop while a message waits for a resumed session to start leaves the session resumed; resuming a fork that has no record forks its original again under `session.fork`'s conditions (FLOW-RESUME-SESSION-R10 added); the open question on merging record and live input is closed (PROTO-AGENT-EVENTS-R24), and one on turns taken elsewhere replaces it; Built so far added, saying a resumed session starts with Core's last values until PROTO-CORE-API-R71's relaunch rule is built.
- 2026-11-13: from #352 inbox: a message sent while the session is being archived is refused before any resume (PROTO-CORE-API-R85). No requirement IDs changed.
- 2026-11-13: ready, from the owner's decisions of 2026-11-13: a project's sessions cover the project's folder and its workspaces' folders, and a session started outside Ersa resumes in its own folder, refused with a reason when the folder is gone. Writing to a stopped session resumes it, as in Claude Desktop, and Resume on the archived note resumes with no message. FLOW-RESUME-SESSION-R01 to R09 added; sessions found in the backend's record don't hold back removing a workspace or project (PROTO-CORE-API, Projects and Workspaces). The tests asserting that resume is a milestone away change with the resume ticket. Today Core refuses every resume; the adapter already lists a folder's sessions and builds `--resume`, and Core already marks a session Core's restart interrupted. Unreachable remote hosts move to `FLOW-REMOTE-SESSION`.
- 2026-11-09: a session whose backend has no adapter in this build is dormant until the adapter is back (ADR 0031).
- 2026-11-01: rewritten for ADR 0017: history is the backend's session record; sessions started elsewhere are listed and resumable; resume by backend id, shown so the user can resume elsewhere; retention is the backend's; live-elsewhere check. Open questions on resume semantics and on which record is replayed answered.
- 2026-10-31: window close no longer stops sessions; transcript from raw log.
- 2026-10-31: created as draft.
