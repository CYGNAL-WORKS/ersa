# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-08 PROTO-CORE-API: `session.list` says a session's `created_at` is the time of the first entry in its record, but a fork's copied entries keep the original's times, so a fork would list as old as the original and sort among its older sessions. Meanwhile a fork's `created_at` is when it was made, kept with its row, once its own record exists too. Decide: name that exception in the brief, or follow the record.

## Resolved recently (optional, trimmed in planning passes)
