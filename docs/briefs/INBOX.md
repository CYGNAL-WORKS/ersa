# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-22 PROTO-CORE-API: nothing says what a put-away refused before git changed anything leaves (a lock on the worktree, local changes, a folder git does not list, no turn at the project's worktrees in 300 seconds, a checkout git cannot read): R127 covers only `workspace.remove` and a failed fork's cleanup, and R124 says only that the sweep leaves the workspace for its next pass. #536 leaves it `ready` with nothing stored, since a `put_away` workspace with its folder still there would have that folder deleted without a check by the next put-away or by a put-back (FLOW-CLEAN-UP-R03); any later failure leaves it `put_away`.


## Resolved recently (optional, trimmed in planning passes)
