# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 PROTO-CORE-API: no notification carries `last_activity_at`, yet FLOW-ORGANIZE-SESSIONS-R10 moves a tile "when Core reports a new last activity"; a turn's end changes the tile's state, so a `session.tile` follows it, but the tile does not hold the time. Meanwhile Core stores and lists it (`session.list`, `session.arrange`) and the rail would have to reread `session.list` on a tile change; deciding whether `session.tile` (or a new notification) carries it is left to the brief.
- [ ] 2026-11-07 PROTO-CORE-API: Sections says nothing of control characters in a section's name (`session.set_title` refuses them in a title), or of how case is ignored beyond ASCII. Meanwhile Core accepts them, trimming only the ends, and compares names by Unicode lowercase, so "É" and "é" are one name.

## Resolved recently (optional, trimmed in planning passes)
