# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 FLOW-SEND-MESSAGE-R07 (#378): COMPOSER.md's status line has no row for a system file or folder dialog that fails to open, and its alert sentence names only a refused message or change. Built meanwhile: the composer says "Ersa couldn't open the dialog to add attachments. Try again." with the danger glyph, announced as an alert, after the backend rows and before a send on its way; it clears when the user next chooses an attach item, attaches anything, or sends, and it is the composer's own, so it goes with the session's composer.


## Resolved recently (optional, trimmed in planning passes)
