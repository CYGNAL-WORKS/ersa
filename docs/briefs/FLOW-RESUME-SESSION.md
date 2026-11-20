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
5. A project's sessions include those the backend recorded in the project's folder or in any of its workspaces' folders, started in the backend's own tools included. They sit in the rail like any other, and resume the same way, in the folder they ran in. A session the backend's own list hides is left out, as Claude Code's hides the runs its SDK made with no one at the keyboard (PROTO-CLAUDE-CODE-STREAM-R79).
6. From a tile's menu, Copy session ID copies the backend's id for the session, so the user can resume it in the backend's own tools. Core holds the id from when it chose it, for a backend that starts a session under an id Core chooses (Claude Code: at the session's creation or fork), else from when the backend names the session or the adapter lists its record. For Claude Code the id names a record only from the session's first message (PROTO-CLAUDE-CODE-STREAM, Invocation): until then the backend's own tools find nothing under it, a fork included, though Ersa shows the fork's conversation from its original's record. An id that leads nowhere is not offered. For an id Core chose, Core gives a window the id only while the adapter finds the backend's record of the session under it (R18), looking when it lists the session and when a turn in it ends (R19), and tells a window that already has the session when that changes (R20, R21). So Copy session ID is unavailable, saying why, on a fork until its first message, and on a new session whose first message never reached the backend. Whether the backend has the record is the backend's to say: Core stores nothing about it.

## Variations and errors

- **The session's folder is gone** (a workspace removed, a folder deleted or moved): Ersa refuses to resume it and says so, in the composer's status line or above the archived note, and the session keeps its history and stays where it is.
- **The session is open in another process** (the backend's own tools): before starting the backend, Ersa says so in the transcript (PROTO-AGENT-EVENTS-R14) and resumes anyway, since the user may mean it; both would write to one record.
- **The backend's record is gone** (its retention removed it): a session started in the backend's own tools leaves the list the next time Ersa lists sessions. A session Ersa started stays, since a new session or a fork has no record until its first message.
- **Removing a workspace or project** with sessions started in the backend's own tools: Ersa forgets those sessions and leaves their records, so they show again in the backend's own tools and if the folder is added again. One the user has resumed in Ersa, and that is still running, holds the removal back until it is stopped, so no backend is left running in a removed folder.
- **Deleting a session** never takes an isolated workspace's files while a session started in the backend's own tools is listed there: it uses the workspace as any other session does (`FLOW-ARCHIVE-DELETE-SESSION`).
- **The backend fails to start**: the message stays in the composer with the reason, as a refused message does (`FLOW-SEND-MESSAGE`), and the session stays stopped.
- **Stop while a message waits for the session to start** cancels the message and leaves the session resumed and idle (PROTO-CORE-API-R86).
- **A fork closed before its first message** has no record of its own, so resuming it forks its original again, under the conditions a fork has (R10). When Core no longer lists the original (it was deleted, or forgotten with its workspace), Core has no session to check those conditions on: it asks the backend to fork from the original's record all the same, and when the backend cannot, as when the record is gone, the resume fails as any start the backend refuses does, with the backend's own words (R11). The agent's conversation in the fork then includes what the original did since, though Ersa shows the fork up to its fork point until its first message gives it a record.
- **A message sent while the session is being archived** is refused, not resumed (PROTO-CORE-API-R85); the user sends again.
- **A session whose backend has no adapter in this build** is dormant until the adapter is back (ADR 0031).

## Requirements

- **FLOW-RESUME-SESSION-R01**: When `session.resume` names a session that is not running, Core shall start its backend again with the session's backend id, in the session's folder, so that the session continues in the same record and its transcript keeps everything before; a session whose backend has no record of it yet (a new session or a fork before its first message) starts again as `PROTO-CLAUDE-CODE-STREAM` (Invocation) says, and a session already running is left as it is.
- **FLOW-RESUME-SESSION-R02**: When the user sends a message to a session that is not running and not archived, Core shall resume the session and then send the message as its first turn, reporting the backend starting as the send's phase.
- **FLOW-RESUME-SESSION-R03**: When a session's folder no longer exists, Core shall refuse to resume it, or to send to it while it is not running, with `invalid_state` and reason `folder_gone`, leaving the session and its arrangement unchanged; Core checks the folder itself before starting the backend.
- ~~**FLOW-RESUME-SESSION-R04**~~: retired 2026-11-15 (sessions the backend's own list hides are left out), replaced by R12.
- **FLOW-RESUME-SESSION-R05**: A session started in the backend's own tools shall keep the same Ersa id across `session.list` calls and Core restarts, so that its arrangement (pinned, section, archived, unseen) is kept.
- **FLOW-RESUME-SESSION-R06**: When the backend's record of a session started in the backend's own tools is gone, `session.list` shall leave the session out from then on.
- ~~**FLOW-RESUME-SESSION-R07**~~: retired 2026-11-19 (an id Core chose is the session's before the backend reports it, and the list and the menu were one statement), replaced by R15 and R16.
- **FLOW-RESUME-SESSION-R08**: When Core starts and finds a session that was live when it last stopped, the session's transcript shall end with its `session.ended` (PROTO-AGENT-EVENTS, Core's lifecycle facts), and the session shall be stopped and resumable.
- ~~**FLOW-RESUME-SESSION-R09**~~: retired 2026-11-15 (one Core is running holds the removal back), replaced by R13 and R14.
- **FLOW-RESUME-SESSION-R10**: When a resume, by `session.resume` or by a send, names a fork that has no record of its own, Core shall fork the fork's original again as `session.fork` does, refusing the resume with `session.fork`'s answer whenever `session.fork` of the original would be refused and holding sends to the original until the fork's backend is ready.
- **FLOW-RESUME-SESSION-R11**: When a resume names a fork that has no record of its own and whose original is not among Core's sessions, Core shall start the fork from the original's record without `session.fork`'s conditions, and when the backend fails to start shall answer as it does for any backend that could not be started.
- **FLOW-RESUME-SESSION-R12**: `session.list` shall include, for each project, every session the backend's adapter lists for the project's folder and for each of its workspaces' folders, those started in the backend's own tools included, each in the workspace whose folder it ran in, counting only records whose own working folder is that folder.
- **FLOW-RESUME-SESSION-R13**: Sessions started in the backend's own tools that Core is not running shall not hold back removing their workspace or project: Core shall forget them, leaving their records.
- **FLOW-RESUME-SESSION-R14**: While Core runs a session started in the backend's own tools, `workspace.remove` of its workspace and `project.remove` of its project shall be refused with `invalid_state`.
- ~~**FLOW-RESUME-SESSION-R15**~~: retired 2026-11-20 (an id the backend has no record under leads nowhere in its own tools; the owner's decision), replaced by R17 to R20.
- **FLOW-RESUME-SESSION-R16**: Copy session ID, in a tile's menu, shall put the session's `backend_session_id` on the clipboard, and be unavailable, with why, while Core has given the window none for the session.
- **FLOW-RESUME-SESSION-R17**: For a session whose id the backend named, on its stream or in a record the adapter listed, Core shall carry its `backend_session_id`, in `session.list` and wherever else it sends the session, from then.
- **FLOW-RESUME-SESSION-R18**: For a session started under an id Core chose, Core shall carry its `backend_session_id` only while the adapter's latest look found the backend's record of the session under that id.
- **FLOW-RESUME-SESSION-R19**: Core shall look for that record when it lists the session and when a turn in the session ends, and shall carry the id from the look that finds it.
- **FLOW-RESUME-SESSION-R20**: Every `session.tile` shall carry the session's `backend_session_id` as `session.list` does, and Core shall send one when what it carries for the session changes.
- **FLOW-RESUME-SESSION-R21**: The Interface shall take a session's `backend_session_id` from `session.tile` as it does from `session.list`.

## Built so far

Core resumes a stopped session on `session.resume` and on a send to it (R01, R02), refuses when its folder is gone (R03), ends the transcript of a session that was live when Core last stopped (R08), and resumes a fork that has no record of its own by forking its original again (R10, R11). A resumed session starts with the model, effort and permission mode Core last asked for, or with the record's latest once a later turn ran with others (PROTO-CORE-API-R71, R98, R99). `session.list` holds the sessions started in the backend's own tools, under ids that last, and removing a workspace or project forgets them (R05, R06, R12 to R14). `session.list` carries each session's backend id from when Core holds it, a self-named one from when the backend names it (R17), and the tile's menu copies it (R16); carrying an id Core chose only while the backend has the session's record, and telling the window when that changes (R18 to R21), is still to build.

## Out of scope

- Remote hosts that can't be reached when Ersa starts (`FLOW-REMOTE-SESSION`, M4).
- A Continue button on an interrupted session: writing "continue" resumes it, as in Claude Desktop.
- Moving a session to another folder or workspace before resuming it.

## Open questions

- Whether a transcript that is open when its session resumes shows turns taken in the backend's own tools since it was opened; today they show when the session is next opened (PROTO-AGENT-EVENTS, Open questions).

## Changes

- 2026-11-20: the owner's decision on the open question: Copy session ID waits until the backend's own tools can find the session. A fork's id was copied from the fork's creation and led nowhere there until the fork's first message. For an id Core chose, Core now carries it only while the adapter finds the backend's record under it (FLOW-RESUME-SESSION-R18, R19), storing nothing of its own about that, and every `session.tile` carries the id, sent when it changes, so a window that already has the session learns it (R20, R21). R15 retired for R17 to R20, R17 keeping what it said of an id the backend named; R21 added; R16 reworded for the window learning the id from the tile as well as the list, meaning unchanged.
- 2026-11-19: from #340 inbox: the id Core chose for a session is the session's from when Core chose it, since the backend runs the session under it and later reports the same one, so `session.list` carries it from when Core holds it, not from when the backend reports it. It names a record only from the first message, so until then the backend's own tools find nothing under it; for a fork, which shows a conversation meanwhile, whether Copy session ID should wait is an open question. FLOW-RESUME-SESSION-R07 retired; R15 (the list) and R16 (the menu) added.
- 2026-11-15: from #328 inbox: sessions the backend's own list hides are left out of `session.list` (FLOW-RESUME-SESSION-R04 retired for R12; the Claude Code rule is PROTO-CLAUDE-CODE-STREAM-R79); a session started in the backend's own tools that Core is running holds back removing its workspace or project (R09 retired for R13 and R14); and such a session counts as using its workspace when another session there is deleted, so the delete leaves the files.
- 2026-11-15: from #354 inbox: a fork with no record of its own whose original Core no longer lists is started from the original's record with no conditions to check, and the backend says what is wrong when it cannot (FLOW-RESUME-SESSION-R11 added). Ersa asks the backend rather than refusing on its own account, since a forgotten original's record may still be there. R10 is built and unchanged.
- 2026-11-14: from #393 inbox: Built so far says a resumed session starts with the record's newer values. No requirement IDs changed.
- 2026-11-13: from #353 inbox: Stop while a message waits for a resumed session to start leaves the session resumed; resuming a fork that has no record forks its original again under `session.fork`'s conditions (FLOW-RESUME-SESSION-R10 added); the open question on merging record and live input is closed (PROTO-AGENT-EVENTS-R24), and one on turns taken elsewhere replaces it; Built so far added, saying a resumed session starts with Core's last values until PROTO-CORE-API-R71's relaunch rule is built.
- 2026-11-13: from #352 inbox: a message sent while the session is being archived is refused before any resume (PROTO-CORE-API-R85). No requirement IDs changed.
- 2026-11-13: ready, from the owner's decisions of 2026-11-13: a project's sessions cover the project's folder and its workspaces' folders, and a session started outside Ersa resumes in its own folder, refused with a reason when the folder is gone. Writing to a stopped session resumes it, as in Claude Desktop, and Resume on the archived note resumes with no message. FLOW-RESUME-SESSION-R01 to R09 added; sessions found in the backend's record don't hold back removing a workspace or project (PROTO-CORE-API, Projects and Workspaces). The tests asserting that resume is a milestone away change with the resume ticket. Today Core refuses every resume; the adapter already lists a folder's sessions and builds `--resume`, and Core already marks a session Core's restart interrupted. Unreachable remote hosts move to `FLOW-REMOTE-SESSION`.
- 2026-11-09: a session whose backend has no adapter in this build is dormant until the adapter is back (ADR 0031).
- 2026-11-01: rewritten for ADR 0017: history is the backend's session record; sessions started elsewhere are listed and resumable; resume by backend id, shown so the user can resume elsewhere; retention is the backend's; live-elsewhere check. Open questions on resume semantics and on which record is replayed answered.
- 2026-10-31: window close no longer stops sessions; transcript from raw log.
- 2026-10-31: created as draft.
