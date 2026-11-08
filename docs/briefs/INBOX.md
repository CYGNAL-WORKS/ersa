# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 LAY-SHELL: no brief says where keyboard focus goes when Pin or Unpin moves a tile to another group (RAIL.md only says a closed menu returns focus to what opened it, and the tile is drawn afresh in its new group). Built meanwhile: focus follows the tile to its new place, or goes to its new group's heading when that group is collapsed.

## Resolved recently (optional, trimmed in planning passes)
