# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 PROTO-CORE-API: R113 names only the 30 seconds and "waiting between tries". #522 waits 250 ms between tries at the folder, doubling to at most 2 s, and moves the last try up to begin at the 30-second mark. It also counts the file-system cap being full (`busy`, no try at the folder began) as ending the step at once, not as a try to repeat. R112 and R113 apply to `session.delete` only; fork cleanup, which shares the code, keeps its four tries and still ends on git's failed removal.


## Resolved recently (optional, trimmed in planning passes)
