# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 FLOW-RESUME-SESSION: R18 says Core carries an id it chose "only while the adapter's latest look found" the record, but not what a look that fails or times out counts as; Core keeps the answer of the last look that concluded, so a slow disk does not flicker Copy session ID. Also, Core tells a chosen id from a named one by the backend (it starts sessions under ids its host chooses) and not by the row, so a Claude Code session from before Core chose ids, whose id the CLI named, is held to R18 rather than R17; it differs only when its record is gone, and then its id leads nowhere anyway.


## Resolved recently (optional, trimmed in planning passes)
