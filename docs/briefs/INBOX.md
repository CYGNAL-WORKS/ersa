# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 FLOW-NEW-SESSION-R07 (COMPOSER.md, The status line): the status line's table has no row for a host with no backend to run, which the new-session screen already treated as unavailable; it keeps the box unavailable and says "No backend is available on this host." on the status line with no leading mark, after detecting backends.

## Resolved recently (optional, trimmed in planning passes)
