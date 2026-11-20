# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 FLOW-NEW-SESSION: no requirement says that nothing is sent while the trust question is open, though R24 to R27 rely on it (a send would start a session on the project chosen before the question). #375 holds send while it is open (send shows unavailable, the words stay editable), and its test cites no requirement.
- [ ] 2026-11-20 none (docs/design/COMPOSER.md, The trust question): COMPOSER.md says what the question shows but not what gives way in a window too small for it. #375 keeps the folder whole wherever the room above the box or the column holds it; in the smallest windows the folder gives up all but its first line and scrolls inside itself, as the explanation does, so the title, the folder and the answers show together.


## Resolved recently (optional, trimmed in planning passes)
