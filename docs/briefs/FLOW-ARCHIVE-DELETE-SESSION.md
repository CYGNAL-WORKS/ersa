---
id: FLOW-ARCHIVE-DELETE-SESSION
type: flow
title: Archive or delete a session
status: ready
references: [LAY-SHELL, PROTO-CORE-API, FLOW-ORGANIZE-SESSIONS]
adrs: [0017, 0026]
---

# Archive or delete a session

## Outcome

A session the user is done with is out of the way: archived, it is stopped and hidden but can be read and resumed; deleted, it is gone from Ersa along with the isolated workspace it alone used and that workspace's branch.

## Trigger

The user chooses **Archive**, **Unarchive** or **Delete** in a tile's menu (`LAY-SHELL`, The rail).

## Steps

1. The user chooses **Archive**. Ersa stops the session, as Stop does, and marks it archived. Its tile leaves the rail unless the rail shows archived sessions (FLOW-ORGANIZE-SESSIONS). Its record and workspace stay.
2. The user opens an archived session (showing Archived or All). Its transcript reads as usual; in place of the composer is a note that it is archived, with **Resume**. Resume unarchives it and resumes it (FLOW-RESUME-SESSION).
3. The user chooses **Unarchive**. The session is no longer archived and returns to its group, stopped; the next message resumes it.
4. The user chooses **Delete**. Ersa asks first. The question names the session and says what goes with it:
   - For a session alone in an isolated workspace: the workspace's folder and its branch, by name, are deleted. When the workspace holds uncommitted or untracked files, ignored files, or commits that no other branch or remote holds (a detached checkout's included), the question says so and how many, and its confirming button reads as deleting them.
   - For a session in the root workspace, or in a workspace another session also uses: the files stay, and the question says where.
5. The user confirms. Ersa stops the session, deletes the workspace and its branch when step 4 said so, and removes the session. Its tile leaves the rail. The backend's own session record stays the backend's (ADR 0026).
6. Cancelling the question leaves everything as it was.

## Variations and errors

- **Deleting fails** (the folder is in use, the branch can't be deleted): the session stays in the rail, stopped, its tile saying the deletion failed (`LAY-SHELL`), and Ersa says what failed and offers **Try again**, which repeats the deletion and asks again only when the workspace has changed since the user confirmed.
- **The workspace changed after the question.** On confirm Ersa stops the session, then checks that what would be lost is what the question described; when anything changed, even with the same counts, Ersa asks again, and the session stays stopped.
- **Ersa stops part way through a delete** (a crash, a restart) after the user confirmed it: the session shows as being deleted and Ersa finishes the delete when it starts again. Stopped before the confirmation was checked, nothing was changed.
- **Another session leaves the shared workspace** between the question and the confirmation, so deleting this one would now delete the files: Ersa asks again.
- **Archive or Delete while the session needs the user**: the pending requests are withdrawn with the stop, as when the session is stopped.
- **The active session** archived or deleted: the transcript area shows the new-session screen.
- **A session started elsewhere** (FLOW-RESUME-SESSION) is archived and deleted the same way; deleting it does not delete its record from the backend's own tools.

## Requirements

- **FLOW-ARCHIVE-DELETE-SESSION-R01**: When the user archives a session, Ersa shall stop it and mark it archived in Core, keeping its record, its workspace and its arrangement.
- **FLOW-ARCHIVE-DELETE-SESSION-R02**: An archived session shall open read-only: Ersa shall show its transcript and offer Resume in place of the composer, and Core shall refuse to send it a message.
- **FLOW-ARCHIVE-DELETE-SESSION-R03**: When the user unarchives a session, or resumes an archived one, Ersa shall clear its archived mark and keep its pin and section.
- **FLOW-ARCHIVE-DELETE-SESSION-R04**: Before deleting a session, Ersa shall ask the user, saying whether its workspace and branch will be deleted, and, when they will, how many uncommitted and untracked files, ignored files, and commits held by no other branch or remote will be lost.
- **FLOW-ARCHIVE-DELETE-SESSION-R05**: When the user confirms, Ersa shall stop the session, delete its isolated workspace's folder and branch when no other session uses that workspace, and then remove the session.
- **FLOW-ARCHIVE-DELETE-SESSION-R06**: Ersa shall never delete, clean or reset a root workspace's files or branch when deleting a session.
- **FLOW-ARCHIVE-DELETE-SESSION-R07**: When deleting the workspace or branch fails, Ersa shall keep the session, stopped, say what failed, and offer to try again.
- **FLOW-ARCHIVE-DELETE-SESSION-R08**: When what a deletion would lose has changed in any way since the user was asked, Ersa shall ask again before deleting.
- **FLOW-ARCHIVE-DELETE-SESSION-R09**: A deleted session shall not appear in the rail again, including after a restart and when Ersa lists the backend's sessions for its project.

## Out of scope

- Deleting a backend's session record.
- Undoing a delete.
- Reclaiming workspaces whose sessions are long gone.

## Open questions

- Whether archiving a session that is working should ask first.

## Changes

- 2026-11-05: created from the owner's decisions of 2026-11-05: Archive stops and hides, opens read-only; Delete removes the session, its isolated workspace and branch after a question that names the work lost, never the root workspace (ADR 0026). R01 to R09 added.
