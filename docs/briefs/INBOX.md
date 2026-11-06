# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 FLOW-APPROVE-PERMISSION: R09's n counts "the requests answered since the shelf opened", but doesn't say whether a request answered elsewhere that the shelf never showed counts. Meanwhile the shelf counts only requests it showed, so one answered elsewhere behind the shown one takes one from m rather than adding one to n ("1 of 3" becomes "1 of 2", not "2 of 3" with the same request still shown).

## Resolved recently (optional, trimmed in planning passes)
