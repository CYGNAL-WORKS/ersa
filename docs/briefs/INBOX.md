# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 FLOW-APPROVE-PERMISSION: R01 shows the oldest request by its event, but a pending request R25 can't show (unknown kind, or no event among those held) has no known place in that order. Meanwhile the shelf counts it oldest only when the events held start after a gap, where its event must have been, and otherwise shows the showable requests first; with none showable it shows the R25 note.
- [ ] 2026-11-06 LAY-SHELL: Built so far still says a tile's chevron is a mark until the shelf is built; it is now the shelf's handle on every tile but the active one's. The rail section also repeats its "Loading, empty and failed" and "The foot" lines under The shelf, in three versions. Code follows FLOW-APPROVE-PERMISSION meanwhile.

## Resolved recently (optional, trimmed in planning passes)
