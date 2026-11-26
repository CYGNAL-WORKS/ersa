# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-26 FLOW-SIGN-IN: `COMPOSER.md` (The sign-in note) has the note open and close upward as the status line does, keeping the box in place; on the new-session screen that puts it over the Ersa wordmark for as long as the backend is signed out, since only `space-xl` lies between them. Meanwhile, on that screen only, the zone above the box takes its place in the column while the note shows (as the trust question already does when room is tight), its height eased over `motion-settle`, and what stands beneath the note (status line, trust question, a folder's line) follows it in the column; the box moves by half the note's height as the column centres again.
