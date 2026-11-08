# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 FLOW-ORGANIZE-SESSIONS: R07 names Previous 7 days without its bounds. #204 puts a session created 2 to 7 local calendar days before today in Previous 7 days, 8 or more in Older, and one created later than now (a clock set wrong) in Today.
- [ ] 2026-11-07 PROTO-CORE-API: Settings names a `no_project` collapsed-group key, but LAY-SHELL (Projects) puts a session whose project Ersa can't tell under no heading, so nothing can collapse it. #204 has no such heading and never writes `no_project`; the key can go, or LAY-SHELL can give those sessions a heading.

## Resolved recently (optional, trimmed in planning passes)
