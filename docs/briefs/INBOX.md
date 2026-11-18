# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-18 FLOW-ORGANIZE-SESSIONS: R10 says Core reports a new last activity "when a turn ends", but PROTO-CORE-API-R75 changes it at other times too: when a live session stops, its last activity goes from its last `turn.completed` `ts` to its record's latest conversation entry (a little earlier, for Claude Code), and work done outside Ersa changes it at the next `session.list` (as the brief's own Last activity bullet says); meanwhile Core follows R75, sends a `session.tile` whenever a record read changes the value (R59), and the rail may move a tile on a stop.

## Resolved recently (optional, trimmed in planning passes)
