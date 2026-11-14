# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-13 FLOW-READ-TRANSCRIPT: R05 says the text before an open construct shall not change, but GFM lets a table interrupt a paragraph: "intro" followed by "| a | b |" reads as one paragraph until the delimiter row arrives, when the header line leaves it for the table. Meanwhile the whole message is parsed again on each piece and React keeps the earlier elements; the table's header line moving into the table is read as the table arriving, not as earlier text changing.
- [ ] 2026-11-13 FLOW-READ-TRANSCRIPT: R02 and TRANSCRIPT.md say an image shows as its alternative text linked to its address, and are silent on an image with no alternative text. Meanwhile it shows its address as the link's text.
- [ ] 2026-11-13 FLOW-READ-TRANSCRIPT: TRANSCRIPT.md (Formatted text) gives Copy no state for a clipboard that refuses the write. Meanwhile Copy stays as it was and says "Copied" only once the code is on the clipboard.

## Resolved recently (optional, trimmed in planning passes)
