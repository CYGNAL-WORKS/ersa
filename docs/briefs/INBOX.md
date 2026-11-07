# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 PROTO-CORE-API: `workspace.changes` leaves out of `uncommitted` a tracked file that is gone from the folder (deleted there, or deleted from the index with the folder matching), since nothing of it is left to lose and a half-removed folder's missing files would otherwise count; such a change still changes `token`. The brief says only "files with changes not committed"; Core counts as described meanwhile.
- [ ] 2026-11-06 PROTO-CORE-API: `workspace.changes` names no time limit; Core gives all its git calls and file reads 60 seconds together, answers `vcs_error` past it, and runs the request detached from the connection's other requests, since it reads every uncommitted file.
- [ ] 2026-11-06 PROTO-CORE-API: `workspace.changes` for a folder git no longer lists as a worktree (git's record of it gone, the folder still there) is not covered; Core answers `vcs_error`, as `workspace.remove` refuses such a folder, since git cannot say what in it is uncommitted.

## Resolved recently (optional, trimmed in planning passes)
