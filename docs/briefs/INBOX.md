# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-22 FLOW-READ-RAW-LOG: R05 asks again at once "when an answer held as many entries as one answer may", but an answer can also stop short at PROTO-CORE-API-R139's 8 MiB, which with `clip` 262144 is about 32 cut lines, and the client can't measure what Core counted. #568 asks again at once only after 1000 entries, so a run of long lines arrives about 32 a second; the brief could have Core say whether more lines wait, or the client ask again at once after any non-empty answer.
