# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-24 FLOW-NEW-SESSION: R48 names a session's backend by its label only while its host's backends are listed, and the brief's Open questions leaves what the row shows before then undecided; with no listing there is no label, only the id, so the backend chip under a session waits for the listing (shows once `backend.list` answers for its host) rather than reading the id. A planning pass decides whether it should instead read the id, or the chip show a detecting state.
