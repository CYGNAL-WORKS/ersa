---
id: FLOW-CLEAN-UP
type: flow
title: Clean up what Ersa leaves behind
status: ready
references: [LAY-SHELL, PROTO-CORE-API, FLOW-ARCHIVE-DELETE-SESSION, FLOW-RESUME-SESSION]
adrs: [0033, 0037, 0038]
---

# Clean up what Ersa leaves behind

## Outcome

The folders and branches Ersa made and no longer needs don't pile up. What holds no unsaved work goes on its own: an archived session's folder is put away, and what a failed start, a failed fork or a crash left is removed. Everything else waits on one list, and nothing on it is removed until the user approves, knowing what each removal loses (ADR 0038).

Archive, Clean up and their settings do what Claude Desktop's do (`claude-desktop/cleanup-2026-11-20.txt`). The sweep, the leftovers and the branches on the list are Ersa's own: nothing read of Claude Desktop says what it does with them.

## Trigger

The user archives a session; Ersa starts, or an hour passes; or the user opens Storage on the settings page (`LAY-SHELL`).

## Steps

1. **Archiving frees the folder.** After a session is archived (FLOW-ARCHIVE-DELETE-SESSION), Ersa puts its isolated workspace away: the folder is removed and the branch stays. A folder that holds unsaved work stays as it is. Nothing is shown; the session reads as any archived one.
2. **Resuming brings it back.** When the user resumes the session, or unarchives it and writes to it, Ersa puts the folder back where it was, checked out as it was, and then resumes (FLOW-RESUME-SESSION). The composer's status line says the folder is being put back.
3. **Ersa sweeps on its own.** When it starts, about every hour, and after a session is archived, Ersa removes each leftover workspace that holds no unsaved work and no ignored files, with its branch, and writes what it removed to the diagnostic log. It asks nothing and shows nothing.
4. **Old sessions archive themselves, when the user says so.** On the settings page, under Storage, **Archive inactive sessions** is Never, or after 1, 2, 7, 14 or 30 days. With a number chosen, the sweep archives each session with no activity for that long, as Archive does, and step 1 follows.
5. **The user cleans up the rest.** Under Storage, **Clean up inactive sessions** takes 30, 60 or 90 days. Ersa lists what it can remove, checking each item as it goes, in groups:
   - the folders of sessions with no activity for that long, and the folders kept after archiving because they hold unsaved work, each with its sessions' titles, its project and when it was last active;
   - leftover workspaces the sweep kept;
   - branches named `ersa/…` that no session uses;
   - folders in Ersa's worktrees folder that Ersa can't identify.
   Each item shows the space it frees, once measured, and what removing it loses: how many changed files, how many commits that exist nowhere else. An item that loses nothing is ticked; so is a branch whose changes the base branch already holds, which says so. The rest are unticked, and ticking one is choosing to discard what it holds.
6. The user chooses to clean up. Ersa asks once, naming how many folders and branches go and the space freed and, when a ticked item holds unsaved work, how many changed files and commits are lost for good; the confirming button then reads as discarding them.
7. The user confirms. Ersa removes the items one at a time, showing how far it is, and the user can stop it after the current one. A session whose folder was removed stays, with its conversation and its branch, and resumes as in step 2. When it is done Ersa says what it removed and what it could not, with why, and reads the list again.

## Variations and errors

- **The branch is gone** when a put-away workspace is put back, or is checked out in another folder: the resume is refused with the reason, and the session stays as it was, archived when it was. So it does when git won't make the folder again for another reason, such as a rebase of the branch under way in another folder: the resume then fails with git's own words.
- **While a folder is being put back**: forking, deleting or removing a session there, and removing the workspace, are refused for the moment, and stopping the message that waits for the folder stops the message at once and lets the folder come back (PROTO-CORE-API-R143, R145).
- **A folder is in use** when Ersa puts it away or removes it: Ersa tries as a delete does (PROTO-CORE-API-R113). The sweep then leaves it for its next pass; the Clean up list says the item could not be removed and why.
- **Something changed after the list was read**: Ersa removes nothing of that item, shows it as it is now, unticked when it now holds unsaved work, and goes on to the next.
- **A session there is running, starting or being deleted**: its folder is not on the list, and is refused if it was.
- **A session shared with the backend's own tools**: a session started there counts as any other, so a folder is put away only when every session in it is archived or inactive.
- **A root workspace** is never put away, listed or removed: it is the user's own folder.
- **A session that is not archived** can be in a put-away workspace: one unarchived and not yet written to, or one whose folder the Clean up list removed. It reads as any stopped session, and its folder comes back when it is resumed.
- **Deleting a session whose workspace is put away** deletes no folder, since there is none, and its branch as a delete always does (FLOW-ARCHIVE-DELETE-SESSION).
- **Forking one** starts the fork's workspace from the branch, or the commit, the put-away workspace had checked out (FLOW-FORK-SESSION).
- **A leftover that holds ignored files** is kept for the list too: the sweep is Ersa's own, so it keeps more than an archive does.
- **Attachments copied into the folder** go with it. A message that named one still shows it in the transcript; the copy is not made again.
- **Ersa stops part way** through putting a folder away or removing one: the next sweep finishes it.
- **The list can't be read**, or an item can't be checked: Storage says so with the reason, and an item that can't be checked is unticked and says its contents can't be counted.
- **Nothing to clean up**: Storage says so.

## Requirements

Putting away and putting back:

- **FLOW-CLEAN-UP-R01**: Core shall put an isolated workspace away when it is `ready`, at least one session uses it, every session that uses it is archived and none is live, starting or dormant, no session marked deleting refers to it, and `workspace.changes` counts no `uncommitted` and no `stranded` and is not `unrecorded`; it shall never put away a root workspace.
- **FLOW-CLEAN-UP-R02**: Putting a workspace away shall store what its folder has checked out (a branch, or a commit when it is on none) and set its state to `put_away` before anything is removed, then remove the folder and git's record of the worktree, keeping the workspace, its sessions and every branch.
- **FLOW-CLEAN-UP-R03**: When `session.resume`, or a send to a session that is not running, names a session whose workspace is `put_away`, Core shall put the workspace back before it checks the folder and before it starts the backend: it shall remove whatever stands at the path, which is what a put-away left unfinished, make the worktree at the same path as `workspace.create` makes one, with the stored branch checked out at its tip, or the stored commit, and set the state to `ready`.
- **FLOW-CLEAN-UP-R04**: When the stored branch or commit no longer exists, Core shall refuse the resume or send with `invalid_state` and reason `branch_gone`; when another worktree has the stored branch checked out, with reason `branch_in_use`; either way it shall leave the workspace `put_away` and the session as it was.
- **FLOW-CLEAN-UP-R20**: When git refuses to make the worktree again for any other reason, as it does for a stored branch that another worktree is rebasing or bisecting, Core shall refuse the resume or send with `vcs_error`, git's own words in `detail`, and shall leave the workspace `put_away` and the session as it was.
- **FLOW-CLEAN-UP-R05**: While a send waits for its session's workspace to be put back, `session.send_progress` shall report phase `putting_back`; the Shell shall keep a `session.resume` waiting for Core's answer for 10 minutes before it counts it unanswered.

The sweep:

- **FLOW-CLEAN-UP-R06**: Core shall sweep when it starts, at least once an hour after that, and when a session is archived. A sweep shall check one workspace at a time, holding at most one of the places PROTO-CORE-API-R116 counts, and no request shall wait for a sweep except for its turn at a project's worktrees (PROTO-CORE-API-R118).
- **FLOW-CLEAN-UP-R07**: A leftover workspace is an isolated workspace that no session refers to and that was made at least an hour ago. When `workspace.changes` counts no `uncommitted`, no `ignored` and no `unshared` and is not `unrecorded`, the sweep shall remove its folder, git's record of the worktree, its branch unless another worktree has it checked out, and the workspace.
- **FLOW-CLEAN-UP-R08**: The sweep shall remove nothing that holds unsaved work and nothing it could not check, shall never delete a session, and shall write to the diagnostic log each workspace it puts away or removes and, the first time it keeps a leftover, why.
- **FLOW-CLEAN-UP-R09**: When a put-away or a removal the sweep began fails, Core shall leave the workspace for the next sweep and tell no client.
- **FLOW-CLEAN-UP-R10**: While the setting `sessions.archive_inactive_days` is 1, 2, 7, 14 or 30, the sweep shall archive, as `session.set_archived` does, each session it lists that is not archived, pinned, unseen or marked deleting, that no connection is subscribed to, and whose last activity, else its creation, is at least that many days ago; unset, it shall archive none. A session `session.set_archived` would refuse is left.

The list:

- **FLOW-CLEAN-UP-R11**: `cleanup.list` shall list as `workspace` each `ready` isolated workspace with at least one session and none live, starting, marked deleting or dormant, whose sessions are all archived, or are all inactive for `inactive_days` with none pinned, unseen or subscribed to; as `leftover` each leftover workspace; as `branch` each local branch named `ersa/…` in a project's repository that no workspace names and no worktree has checked out; and as `folder` each folder in a project's folder under the worktrees root that no workspace and no git worktree record names.
- **FLOW-CLEAN-UP-R12**: `cleanup.inspect` shall report, for a `workspace` or `leftover`, what `workspace.changes` counts and the folder's size; for a `branch`, its `unshared` commits and whether merging it into the base branch would change nothing (`merged`); for a `folder`, its size; and a `token` that changes whenever what removing the item would lose changes.
- **FLOW-CLEAN-UP-R13**: `cleanup.remove` shall put a `workspace` item away; remove a `leftover` item's folder, git's record, branch and workspace; delete a `branch` item's branch; and delete a `folder` item's folder. It shall refuse with `invalid_state`, removing nothing, when `token` differs from the item's token now, with the item as it is now in `detail`, and when removing the item would lose unsaved work, or the item is a `folder`, and `discard` is not true.
- **FLOW-CLEAN-UP-R14**: Nothing in this flow shall delete a session or its record, change a root workspace, delete a branch on a remote, delete a local branch not named `ersa/…` that no workspace names, or delete a folder outside the worktrees root that no workspace names.

Storage:

- **FLOW-CLEAN-UP-R15**: Storage shall offer Archive inactive sessions as Never, 1, 2, 7, 14 or 30 days, showing the value in force, and shall write the choice to `sessions.archive_inactive_days`, Never clearing it.
- **FLOW-CLEAN-UP-R16**: The Clean up list shall show each item with what it is, its sessions' titles and project where it has them, the space it frees once measured, and how many changed files and commits that exist nowhere else removing it loses; it shall tick by default exactly the items whose removal loses no unsaved work and the `branch` items reported `merged`, and never a `folder`.
- **FLOW-CLEAN-UP-R17**: Ersa shall remove nothing on the list before the user confirms one question that names how many folders and branches go, the space freed and, when a ticked item holds unsaved work, how many changed files and commits are lost for good.
- **FLOW-CLEAN-UP-R18**: After the user confirms, Ersa shall remove the ticked items one at a time, show how many are done, stop before the next item when the user stops it, say for each item it could not remove why, and read the list again when it ends.
- **FLOW-CLEAN-UP-R19**: Storage shall say that it is checking while the list is read and each item until it is inspected, that there is nothing to clean up when the list is empty, and why when the list can't be read or an item can't be inspected, leaving that item unticked.

## Out of scope

- Selecting several sessions and archiving or deleting them at once. The owner wants it (2026-11-21), it is what removes old sessions' branches, and it gets its own planning pass.
- Archiving a session when its pull request closes: Ersa knows nothing of pull requests yet.
- Archiving a session and keeping its folder, which Claude Desktop's "by default" implies it can do.
- A breakdown of what Ersa uses on disk.
- A session with no record that a start cut off after `session.create` left in the rail: the user deletes it.
- Removing a project, and rotating the diagnostic log while Ersa runs.
- What a backend keeps about a folder Ersa removed (ADR 0033).

## Open questions

- Whether an hour is the right bound before a workspace no session uses counts as a leftover. It only has to outlast the moment between making a workspace and starting its session.
- Whether a folder that holds ignored files Ersa did not make, such as local configuration that was never committed, should be kept as one that holds unsaved work is (ADR 0038, Consequences).
- Whether putting a workspace back should refuse when it finds files at the path that the put-away did not leave. Today Core can't tell them apart, and the path is the workspace's own.
- Whether Ersa should say, on an archived session, that its folder was kept and why. Today only the Clean up list does.
- What Claude Desktop does when a resumed session's branch is gone. The fixture did not observe it; Ersa refuses with the reason.

## Changes

- 2026-11-24: from #537 inbox: R04 names `branch_in_use` for a branch another worktree has checked out, and git also refuses a branch another worktree is rebasing or bisecting, which it lists as on no branch. That resume answers `vcs_error` with git's own words, which name the other folder, and leaves the workspace `put_away` (FLOW-CLEAN-UP-R20): telling it apart would mean reading git's own files or matching its message, for a case that takes a person rebasing an `ersa/…` branch by hand in another folder. What a put-back holds and what stopping a message does during one are PROTO-CORE-API-R143 to R145, which a variation now points at. From #582's merge review: the second clause of R05, the Shell's 10-minute wait for `session.resume`, states what the Shell already did when this brief was created, so #537 pins it with a test and builds only the first. R20 added; no requirement retired or reworded.
- 2026-11-21: created from the owner's direction of 2026-11-21: start from what Claude Desktop does, and put it on the settings page. Archiving puts the workspace away, which replaces his decision of 2026-11-05 that it stays (FLOW-ARCHIVE-DELETE-SESSION-R01 retired there); the sweep removes without asking only what holds no unsaved work (ADR 0038); the rest is one list. Settled in planning: ignored files are not unsaved work, as in Claude Desktop, though the sweep keeps a leftover that holds any; a put-away keeps what was checked out, so a session whose agent changed branches comes back as it was; the list removes one item per request, so nothing new reports progress; acting on several sessions at once is its own planning pass. FLOW-CLEAN-UP-R01 to R19 added.
