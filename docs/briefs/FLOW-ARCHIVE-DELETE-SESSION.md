---
id: FLOW-ARCHIVE-DELETE-SESSION
type: flow
title: Archive or delete a session
status: ready
references: [LAY-SHELL, PROTO-CORE-API, FLOW-ORGANIZE-SESSIONS]
adrs: [0017, 0026, 0031]
---

# Archive or delete a session

## Outcome

A session the user is done with is out of the way: archived, it is stopped and hidden but can be read and resumed; deleted, it is gone: its conversation is deleted from the backend too, along with the isolated workspace it alone used and that workspace's branch, unless the branch holds commits that exist nowhere else, which keeps it.

## Trigger

The user chooses **Archive**, **Unarchive** or **Delete** in a tile's menu (`LAY-SHELL`, The rail).

## Steps

1. The user chooses **Archive**, which is unavailable, saying to stop the session first, while the session is starting, working or waiting on the user. Ersa stops the session, as Stop does, and marks it archived. Its tile leaves the rail unless the rail shows archived sessions (FLOW-ORGANIZE-SESSIONS). Its record and workspace stay.
2. The user opens an archived session (showing Archived or All). Its transcript reads as usual; in place of the composer is a note that it is archived, with **Resume**. Resume unarchives it and resumes it (FLOW-RESUME-SESSION).
3. The user chooses **Unarchive**. The session is no longer archived and returns to its group, stopped; the next message resumes it.
4. The user chooses **Delete…**, which is unavailable, saying to stop the session first, while the session is starting, working or waiting on the user. Ersa asks first. The question names the session, says its conversation is deleted for good, in the backend's own tools too, and says what else goes with it:
   - For a session alone in an isolated workspace: the workspace's folder is deleted, and its branch, by name, unless the branch holds commits that no other branch or remote holds, when the question says the branch is kept and why. When the folder holds uncommitted or untracked files, ignored files, or commits no branch holds (made on a detached checkout), the question says so and how many, and its confirming button reads as deleting them.
   - For a session in the root workspace, or in a workspace another session also uses: the files stay, and the question says where.
5. The user confirms. Ersa stops the session, deletes the workspace, and its branch, as step 4 said, deletes the session's record from the backend, and removes the session. Its tile leaves the rail (ADR 0026).
6. Cancelling the question leaves everything as it was.

## Variations and errors

- **Resume is refused**, as it is until resuming is built (milestone M1): the session stays archived and Ersa says why; Unarchive still returns it to its group.
- **Deleting fails** (the folder is in use, the branch can't be deleted): the session stays in the rail, stopped, its tile saying the deletion failed (`LAY-SHELL`), and Ersa says what failed and offers **Try again**, which asks the question again with what is left, then repeats the deletion from where it stopped.
- **The workspace changed after the question.** On confirm Ersa stops the session, then checks that what would be lost is what the question described; when anything changed, even with the same counts, Ersa asks again, and the session stays stopped.
- **Ersa stops part way through a delete** (a crash, a restart): when it starts again, a delete the user had confirmed shows as failed and offers Try again; one stopped before the confirmation was checked deleted and marked nothing.
- **The session can't be stopped**: Ersa says so and deletes nothing.
- **Another session leaves the shared workspace** between the question and the confirmation, so deleting this one would now delete the files: Ersa asks again.
- **The active session** archived or deleted by the user in this window: the transcript area shows the new-session screen. A shown session that becomes archived otherwise, as when it is archived in another window, stays shown with the archived note, as an archived session opened on purpose does.
- **At launch**, the window opens the newest session that is not archived, since the rail hides archived ones.
- **A session started elsewhere** (FLOW-RESUME-SESSION) is archived and deleted the same way, and deleting it deletes it from the backend's own tools too.
- **The session is open in another process**, such as the backend's own terminal: Ersa has stopped the session by then, since it can't otherwise tell the other process from its own; it says where, deletes nothing, and the session stays stopped, since the other process would go on writing its record.

## Requirements

- **FLOW-ARCHIVE-DELETE-SESSION-R01**: When the user archives a session, Ersa shall stop it and mark it archived in Core, keeping its record, its workspace, its pin and its section.
- **FLOW-ARCHIVE-DELETE-SESSION-R02**: An archived session shall open read-only: Ersa shall show its transcript and offer Resume in place of the composer, and Core shall refuse to send it a message.
- **FLOW-ARCHIVE-DELETE-SESSION-R03**: When the user unarchives a session, or resumes an archived one and Core accepts the resume, Ersa shall clear its archived mark and keep its pin and section.
- **FLOW-ARCHIVE-DELETE-SESSION-R04**: Before deleting a session, Ersa shall ask the user, saying whether its workspace and its branch will be deleted; how many uncommitted and untracked files, ignored files, and commits no branch holds will be lost with the folder; and, when the branch is kept, that it holds commits that exist nowhere else.
- **FLOW-ARCHIVE-DELETE-SESSION-R05**: When the user confirms, Ersa shall stop the session, delete its isolated workspace's folder when no other session uses that workspace, delete the workspace's branch only when it holds no commit that no other branch or remote holds, and then remove the session; while the session is starting, working or waiting on the user, Ersa shall offer Delete as unavailable with the reason.
- **FLOW-ARCHIVE-DELETE-SESSION-R06**: Ersa shall never delete, clean or reset a root workspace's files or branch when deleting a session.
- **FLOW-ARCHIVE-DELETE-SESSION-R07**: When deleting the workspace or branch fails, Ersa shall keep the session, stopped, say what failed, and offer to try again.
- **FLOW-ARCHIVE-DELETE-SESSION-R08**: When what a deletion would lose has changed in any way since the user was asked, Ersa shall ask again before deleting.
- **FLOW-ARCHIVE-DELETE-SESSION-R09**: When the user confirms a delete, Ersa shall delete the session's record from its backend, so that neither the rail, after a restart or when Ersa lists the backend's sessions, nor the backend's own tools list it again.
- **FLOW-ARCHIVE-DELETE-SESSION-R10**: When `workspace.changes` reports `unrecorded`, the delete question shall say that commits made on a detached checkout can't be counted and may be lost with the folder, in place of a count of them.
- **FLOW-ARCHIVE-DELETE-SESSION-R11**: While the session is starting, working or waiting on the user, Ersa shall offer Archive as unavailable with the reason, as Claude Desktop does, and Core shall refuse `session.set_archived` with `archived: true`.

## Out of scope

- Undoing a delete.
- Reclaiming workspaces whose sessions are long gone.

## Changes

- 2026-11-09: owner's decision from the audit against Claude Desktop: Archive is unavailable while the session is starting, working or waiting on the user, which closes the open question on asking first. R11 added.
- 2026-11-07: from #261 inbox: the active session goes to the new-session screen only when the user archives or deletes it in this window, and launch opens the newest session that is not archived; COMPOSER.md draws the archived note and RAIL.md an archived tile's line two, a refused Archive or Unarchive, and focus when a tile leaves the rail. No requirement IDs changed.
- 2026-11-07: from #252 inbox: a delete refused because the session is open in another process leaves the session stopped. No requirement IDs added or retired.
- 2026-11-07: from #244 inbox: when git has lost a workspace's record, the delete question says commits made on a detached checkout can't be counted (R10).
- 2026-11-06: from #236 inbox: a refused Resume leaves the session archived (PROTO-CORE-API-R43); R03 reworded to say a resume clears the mark once Core accepts it, its meaning kept.
- 2026-11-05: from #174 merge review: R01 names the pin and the section it keeps, since Arrangement now covers more. Meaning unchanged.
- 2026-11-05: owner, before this shipped: Delete deletes the session's conversation and record from the backend, keeps a branch with commits that exist nowhere else, and is unavailable while the session is starting, working or waiting on the user. R04, R05 and R09 reworded; no test cites them.
- 2026-11-05: created from the owner's decisions of 2026-11-05: Archive stops and hides, opens read-only; Delete removes the session, its isolated workspace and branch after a question that names the work lost, never the root workspace (ADR 0026). R01 to R09 added.
