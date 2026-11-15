# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 FLOW-ORGANIZE-SESSIONS: R07, R09 and R10 say a corrected date created (such as a fork moving to its original's date once it has its own record) moves a tile, but no notification carries `created_at`: Core changes it silently when it reads a record, and #329 retires the turn's-end `session.list` reread that used to catch it alongside the title. Meanwhile the Interface shows a corrected date at its next list read (a new session, a change, a reconnect). Decide whether `session.tile` carries `created_at` (or Core raises a tile when it changes), or the requirements drop the promise.




## Resolved recently (optional, trimmed in planning passes)
