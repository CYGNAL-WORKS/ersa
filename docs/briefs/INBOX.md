# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 PROTO-CORE-API: git cannot add, remove or prune two worktrees of one repository at once (one lists the others and dies on a record still being written, "failed to read .git/worktrees/<w>/commondir"; #527), and the brief does not say Core runs one change to a project's repository's worktrees at a time. #527 holds, never refuses: `workspace.create` (and the workspace `session.fork` makes), `workspace.remove`, `session.delete`'s step 5 and a failed fork's cleanup each wait, in the order they asked, while another of them is adding, removing or pruning a worktree in the same project; a delete keeps the turn for git's removal, lets it go while it tries the folder, and waits again for its prune and the branch. A request waits at most 300 seconds, counted from when it began to wait for its turn; past that it starts no git work and answers `vcs_error` "another change to this project's workspaces did not finish in time; try again" (`workspace.remove` then leaves the workspace as it was; a delete fails step 5). This narrows R92: a request sent after one of these, on the same connection, that changes the same repository's worktrees waits for it. The Shell now waits 10 minutes for `session.fork`, as for `workspace.create`. A person or agent running `git worktree` in the repository themselves can still race Core (ADR 0037 leaves outside races out). No test cites a requirement for this yet: `a_second_workspace_made_at_once_waits_for_the_firsts_git` (workspaces.rs) and the admission table's worktree tests would.


## Resolved recently (optional, trimmed in planning passes)
