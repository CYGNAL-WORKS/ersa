# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-20 FLOW-ARCHIVE-DELETE-SESSION: a Delete on Windows, run seconds after the session stopped, removed git's record of the worktree and then left about 7,000 files (mostly `node_modules`), so the session read "Couldn't delete". Nothing held the files minutes later, and Core's own folder removal then deleted them cleanly, so the likely cause is files held briefly by processes that outlived the session. Two things the brief leaves open: how long a delete keeps retrying busy files (four tries over about 2 seconds today, too short when a stopped session's child processes take longer to exit), and whether Core logs why a delete failed (it logs nothing, so the cause could only be inferred). Meanwhile, nothing changed; the user's try again finishes the delete.


## Resolved recently (optional, trimmed in planning passes)
