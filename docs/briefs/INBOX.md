# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-19 PROTO-CORE-API: when step 5 or 6 of `session.delete` reaches its time limit while its call is stuck (a record deletion or a folder removal on a disk that stopped answering), the brief says Core "releases the claim" as it answers; #369 keeps the claim until that call has really ended, so a try again meanwhile answers `invalid_state` `busy` and never starts a second deletion beside the stuck one. What this means for users: while the call stays stuck, which on a dead drive can be until Core restarts, the brief's claim also refuses `busy` a `session.create`, `session.fork` or `session.remove` in that workspace, a project's root workspace included, so no new session starts in that project's own folder until then. The branch holds the claim this way (the delete's admission operation, shared with the call); to decide: reword step 5/6 and R91 to say the claim is released once the called-off step has ended, and whether the root workspace's other sessions should be held that long. No requirement states the hold, so the tests for it cite none.


## Resolved recently (optional, trimmed in planning passes)
