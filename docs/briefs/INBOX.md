# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-22 PROTO-CORE-API: nothing says what happens on a workspace while a put-back holds it (#537). The code does what it does during a put-away: `session.fork`, `session.delete` and `session.remove` of a session there and `workspace.remove` are refused busy, and a session found in the backend's records for its folder is not listed into it until the put-back ends, though the store still says `put_away`, from which a fork is allowed (R123). A put-back is refused busy while a delete of a session there holds the workspace.
- [ ] 2026-11-22 PROTO-CORE-API: `session.interrupt` cancels a send "while checking, copying, or waiting for the backend to start" and says nothing of putting back (#537). The code cancels it then too: the send answers `send_stopped` at once and starts no backend, and the put-back runs to its end by itself, so no worktree is left half made; the workspace is then `ready`, or still `put_away` when the put-back is refused or fails.
- [ ] 2026-11-22 FLOW-CLEAN-UP: R04 names `branch_in_use` for a branch another worktree has checked out; git also refuses a branch another worktree is rebasing or bisecting, which `git worktree list` shows as on no branch (#537). That resume answers `vcs_error` with git's words, not `branch_in_use`.
