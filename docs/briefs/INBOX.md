# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 PROTO-CORE-API: `workspace.changes` leaves out of `uncommitted` a tracked file that is gone from the folder while the index holds nothing the commit does not (deleted from the folder alone, or the deletion staged), since nothing of it is left to lose and a half-removed folder's missing files would otherwise count; a file staged and then deleted still counts, and either change still changes `token`. The brief says only "files with changes not committed"; Core counts as described meanwhile.
- [ ] 2026-11-06 PROTO-CORE-API: `workspace.changes` names no time limit; Core gives all its git calls and file reads 60 seconds together, answers `vcs_error` past it, and runs the request detached from the connection's other requests, since it reads every uncommitted file.
- [ ] 2026-11-06 PROTO-CORE-API: `workspace.changes` for a folder git no longer has a record of (on Windows a failed removal deletes git's record and leaves files it could not delete) is not covered; Core counts every file left that the ignore rules do not ignore as uncommitted, since git cannot say which were committed, and counts the branch's commits only.

## Resolved recently (optional, trimmed in planning passes)
