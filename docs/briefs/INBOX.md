# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-09 PROTO-CORE-API, FLOW-FORK-SESSION-R06: owner's decision: a fork's title and creation time are the backend's, and Ersa stores neither (the rule the briefs already give every session). A fork is named through the backend, as a user's rename is, and Core shows what the backend says. So a fork never sent a message has no title after a restart, since the CLI keeps no record of it, and `created_at` is the first entry in the fork's record once it has one, which is the original's date. `session.fork` still says `session.list` reports the marked title until the fork's own record gives one, and describes a naming the backend refuses; both need rewording.

## Resolved recently (optional, trimmed in planning passes)
