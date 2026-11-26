# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-26 FLOW-CLEAN-UP: R12 asks for a `folder` item's `token` to change whenever what removing it would lose changes, and PROTO-CORE-API stops measuring a folder at 30 seconds; a folder too big to walk in that time can't have a token that follows everything in it. Meanwhile (#542) such a folder's token is made new on every inspect, so it never matches and `cleanup.remove` (#543) would refuse it as changed every time: nothing is removed on what the user did not see, and a folder that large can't be removed from the list at all. A planning pass should say what a folder too big to measure should do: stay unremovable, be removed on the user's word with what can't be counted said plainly, or have more time.
