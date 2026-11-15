# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 LAY-SHELL: COMPOSER.md says what an empty context ring says only on a session (context isn't reported yet); the new-session screen's ring keeps "Context: none used yet", since nothing exists to report. The bands are read on the whole percent the ring shows, so 50.4 percent (reading 50 percent) is quiet. #410 builds both; confirm or change.




## Resolved recently (optional, trimmed in planning passes)
