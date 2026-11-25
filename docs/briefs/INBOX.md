# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-24 FLOW-READ-RAW-LOG: R07 says the list says how many lines arrived since the user scrolled away, and WORKBENCH.md shows only "3 new"; nothing says what Jump to latest shows before any line has arrived. The branch shows it with no count, named "Jump to latest", so "0 new" never shows.
- [ ] 2026-11-24 FLOW-READ-RAW-LOG: WORKBENCH.md draws keyboard focus only on the selected row, but the list must take focus as one stop (R17) and Jump to latest sends focus to it when it goes, before any row can be selected. The branch draws the list's focus as a control without an edge does, an `ink` hairline inside the list's area; #570 may keep or replace it when selection comes.
- [ ] 2026-11-24 FLOW-READ-RAW-LOG: WORKBENCH.md says a row draws at most the line's first 2,000 characters without saying what a character is. The branch counts UTF-16 code units, dropping a last unit that would split a character outside the Basic Multilingual Plane; a grapheme count would cost a segmenter on every row drawn.
