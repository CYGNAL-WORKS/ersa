# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-25 FLOW-READ-RAW-LOG: WORKBENCH.md gives the pane one problem line for both problems (the raw log can't be read; the diagnostic log couldn't be opened) but says neither which shows when both stand nor when the second goes, since nothing retries it. Built meanwhile (#571): the open failure shows in place of a read failure while both stand, as it answers what the user just did, and the read failure shows again once it goes; it goes when a later Open diagnostic log opens the file, or when the pane closes. Also R13's live check can't open a file in the live harness, whose window is always attached to a label, where R13 makes the button unavailable: the live check covers the unavailable state, and the open was driven by hand.
