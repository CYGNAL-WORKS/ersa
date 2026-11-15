# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 FLOW-SEND-MESSAGE (COMPOSER.md, Stop): nothing says how stop looks once chosen, while the turn it stops winds down, nor that a second choice is ignored (#283: repeated clicks sent repeated interrupts, and under R20 a late one would stop the waiting message's turn). Built meanwhile: the same square with its icon in `ink-faint`, unchanged under the pointer, `aria-disabled`, named "Stopping…", focus kept; it takes stop again once a turn starts or ends.
- [ ] 2026-11-14 FLOW-SEND-MESSAGE-R08: nothing says what pasting a clipboard that holds both text and an image does (copied cells carry both). Built meanwhile: the image is attached and the text pastes too.
- [ ] 2026-11-14 FLOW-SEND-MESSAGE-R19 (COMPOSER.md, The status line): the order sentence above the table still ends "then a message waiting for the turn to end", where the table row and R19 say it waits in the backend. Also, CONTENT.md's ellipsis rule leaves no way to mark a name cut to its first words. Built meanwhile: the waiting row is last in the order; the name is the first 6 words with no ellipsis ("Waiting: {name}"), and the count reads "{count} messages waiting".



## Resolved recently (optional, trimmed in planning passes)
