# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-25 PROTO-CORE-API: a replay capture's `sessions` is described as "`session.list` on attach", but the first listing on attach may or may not show the session's first event yet (`last_event_seq` is absent before it), so a capture taken straight after attaching changed by chance (#642). The Shell's replay test now takes the first listing once Core lists the session's start; the sentence could say so. The same sentence says `notifications` is every notification, but the test leaves out `session.tile`, whose count and place vary.
