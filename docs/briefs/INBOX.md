# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 FLOW-APPROVE-PERMISSION: nothing says what the shelf shows when every request it knew of is answered (by their events) before it showed one, while the tile still lists them and R08 has not closed it yet. Closing it at once would put focus on the chevron, which goes a moment later when Core lists the session, so built: the shelf stays open and says "Every request from this session is answered." as one of the rail's notes (`rail.shelf.answeredNote`), in the caution tone, until R08 closes it; `RAIL.md` (The shelf: Notes) lists no such note and names no tone for a shelf showing no request.


## Resolved recently (optional, trimmed in planning passes)
