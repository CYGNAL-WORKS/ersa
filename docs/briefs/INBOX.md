# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-21 PROTO-CORE-API: Worktree changes says a `workspace.remove` whose wait for a turn runs out "leaves the workspace as it was", but a removal that deleted the folder waits again for its prune and the branch (Removing a folder), and that second wait can run out with the folder gone. #535 answers `vcs_error` and leaves the workspace `error`, as for any other failure after git began, so no session is created in a `ready` workspace with no folder.
- [ ] 2026-11-21 PROTO-CORE-API: R122 says the cleanup of a failed fork removes the folder as a delete does, and that the cleanup leaves the workspace `ready` when its turn runs out (R119), but not what a cleanup that failed after git began leaves. #535 leaves it `error`, as a failed `workspace.remove` does, and `ready` only for a refusal before git touched it (no turn in time, a lock).
- [ ] 2026-11-21 PROTO-CORE-API: R107 keeps a delete's claim while a try at the folder it gave up on still runs, so a retry begins no second removal beside it; nothing says whether `workspace.remove` and a failed fork's cleanup, which now make the same tries (R122), hold their removal that long. #535 holds it, as a delete does: until that try's thread returns, a `workspace.remove` of the workspace is refused `invalid_state` (busy), and the removal then ends as failed, leaving the workspace `error`.


## Resolved recently (optional, trimmed in planning passes)
