# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-17 FLOW-NEW-SESSION-R31: "for as long as the window stays open" can be read as ending when the window is closed, which R01 makes a hide, or as lasting while Ersa runs; and nothing says whether trusting the project again ends the note. #316 keeps the note until Ersa quits (closing and reopening the window keeps it; it is never saved) and drops it while the project is trusted again.

## Resolved recently (optional, trimmed in planning passes)
