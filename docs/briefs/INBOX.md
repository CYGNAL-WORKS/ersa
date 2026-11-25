# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-24 FLOW-READ-RAW-LOG: no requirement says what happens to the selection and its detail when the selected line leaves the list. Built (#570): the selection follows the line by its index, so a line filtered out or scrolled away stays selected with its detail open; a line the pane lets go of (past 50,000, or an answer that starts later, R09) keeps its detail open until the user closes it or selects another; when the reader starts over (R18) the selection clears.
- [ ] 2026-11-24 FLOW-READ-RAW-LOG: R17 says Up and Down "move the selection by a row" but not what they do with no row selected. Built (#570): Down selects the first row wholly in view and Up the last, so the user stays where they were reading rather than being sent to the first or the 50,000th line.
