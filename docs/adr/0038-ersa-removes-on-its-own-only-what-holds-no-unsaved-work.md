# 0038: Ersa removes on its own only what holds no unsaved work

- Status: accepted
- Date: 2026-11-21
- Related briefs: FLOW-CLEAN-UP, FLOW-ARCHIVE-DELETE-SESSION, FLOW-RESUME-SESSION, PROTO-CORE-API

## Context

Every session Ersa starts in a git project gets an isolated workspace: a folder and a branch. Only Delete removed one, a session at a time, and an archived session kept its folder for good (the owner's decision of 2026-11-05). Around that, folders, branches and rows were left with nothing to remove them: the workspace of a start cut off before its session existed, of a fork whose cleanup failed, of a removal a crash interrupted, and the branch a delete had to keep. None of these shows anywhere in Ersa. The owner asked on 2026-11-21 for a way to clean up old branches, forks and the rest, starting from what Claude Desktop does (`docs/briefs/protocol/fixtures/claude-desktop/cleanup-2026-11-20.txt`).

Anything that deletes files without a question can lose a person's work, and the people Ersa is for include those who would not know where to look for it (`PRD-ERSA`, Plain for everyone). So what Ersa may remove unasked had to be settled once, for every place that removes.

## Options considered

1. **Remove nothing unasked.** Every folder and branch stays until the user approves its removal. Safe, and it leaves the user a list of things they never made and can't judge: the leftovers of a failed fork mean nothing to them.
2. **Remove by age.** Anything older than some number of days goes. Simple to say, and it deletes uncommitted files the moment they are old enough.
3. **Remove unasked only what holds no unsaved work, and ask for the rest.** What decides is what would be lost, never how old it is. Age only ever archives a session, and only when the user turned that on.

## Decision

Option 3.

- **Unsaved work** is what removing a folder or a branch would lose for good: files with uncommitted changes, untracked ones included, and commits that no other branch or remote holds. Ignored files are not unsaved work: version control was told they don't matter, and they are most of what a folder weighs.
- **Archiving puts the workspace away.** Once every session that uses an isolated workspace is archived and its folder holds no unsaved work, Core removes the folder and keeps the branch; resuming a session there puts the folder back at the same path. A folder that holds unsaved work stays. This replaces the decision of 2026-11-05 that an archived session's workspace stays, and is what Claude Desktop does.
- **Core sweeps on its own.** At start and from time to time it removes the workspaces no session uses when they hold no unsaved work, with their branches, and writes what it removed to the diagnostic log. It never asks, so it never removes unsaved work, and it leaves alone anything it can't be sure of: no vendor shows what to do here, so the sweep also keeps a leftover that holds ignored files.
- **Everything else is asked.** What holds unsaved work, branches no session uses, folders Ersa can't identify, and the folders of sessions that have gone unused go on one Clean up list. Nothing on it is removed until the user approves, the list says what each removal loses, and there is no undo.
- **A session is deleted only by Delete.** Cleaning up frees folders and removes leftovers; it never deletes a conversation.
- **Local only.** Ersa never deletes a branch on a remote.
- **The backend's own data stays the backend's** (ADR 0033): what a backend keeps about a folder Ersa removed is under the backend's retention.

## Consequences

- An archived session no longer costs disk space once its changes are committed. Resuming one takes as long as checking its branch out again, and what its folder held outside version control (build output, installed packages, the copies of attachments) is gone and is made again by whoever needs it.
- A session's folder can be absent while the session is fine, so "the folder is gone" stops meaning something is wrong: a workspace has a state for it, and resume puts the folder back before it checks.
- Every removal of a workspace's folder, by a delete, a failed fork's cleanup, the sweep or the Clean up list, is the same removal with the same tries. The three that differed become one.
- The one-hour bound that keeps the sweep away from a start still under way means a leftover lives up to an hour. That is the price of not tying a workspace to the window that made it.
- A branch whose commits exist nowhere else is never deleted unasked, so squash-merged branches stay until the user approves the list, which marks the ones the base branch already holds.
- Deleting many old sessions at once, which is what removes their branches, is not part of this: selecting several sessions and acting on them is planned on its own.
- Revisit if people lose ignored files they needed (local configuration that was never committed), which would argue for keeping a folder that holds ignored files Ersa did not make.
