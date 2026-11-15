# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 FLOW-RESUME-SESSION-R10: when a fork with no record of its own is resumed and its original is no longer among Core's sessions (deleted, or forgotten), R10 can be read either way: refuse as `session.fork` of an unknown id does (`not_found`), or fork the original's record anyway with no checks to pass. Meanwhile Core forks it anyway and the backend says what is wrong (a deleted original has no record to copy).





## Resolved recently (optional, trimmed in planning passes)
