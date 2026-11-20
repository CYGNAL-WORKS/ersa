# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 PROTO-CORE-API: R22 says `session.send` keeps `.ersa/` ignored by version control, and the Turns and attachments section names `.ersa/.gitignore` (written with `*` when absent) as the only ignore file Core writes and refuses as a link. When the repository ships its own `.ersa/.gitignore` that ignores less, Core can make all of `.ersa/` ignored only by rewriting a file the user tracks. Meanwhile (#371) Core leaves that file as it is and also writes `.ersa/attachments/.gitignore` holding `*` when absent, refusing the message when that path is a link or anything but a plain file; git applies the deeper file, so every copy stays ignored. Proposed: R22 says "keep the scratch folder ignored by version control", and the section names the second file and its refusal.


## Resolved recently (optional, trimmed in planning passes)
