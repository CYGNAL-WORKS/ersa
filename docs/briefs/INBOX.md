# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: R76 and the `rate_limit_event` section say a line with no `unifiedWindows` reports no windows, but not what an empty `unifiedWindows` object reports (no recording has one). The adapter projects it as `windows: []`, a report that the login now counts against no window, since a report's list replaces the last one (#421).





## Resolved recently (optional, trimmed in planning passes)
