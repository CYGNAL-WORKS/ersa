# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CORE-API: the open question whether folder inspections for `project.add` and `project.find` take a place under a cap is answered in code (#396, from Docket's review): at most 8 under way at once across Core, each keeping its place on its thread until its file system call returns, past the 10-second limit too, as `attachment.inspect` reads do; with none free, both answer `permission` at once, saying too many folders are not answering. The brief should state the cap and that answer, and close the open question.



## Resolved recently (optional, trimmed in planning passes)
