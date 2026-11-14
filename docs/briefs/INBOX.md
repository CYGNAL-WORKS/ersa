# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM-R47: R47 does not say which entries mark "a turn that began after a time" nor whether a subagent's entries count. Built meanwhile: a turn's start is a `user` entry carrying `permissionMode` (only prompts carry it, stamped when their turn starts, fixtures 22, 23), a reply's turn began at the latest such entry not after it, and `isSidechain` entries are skipped, since a subagent's model is not the session's.
- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM-R47: R47 gives one time, but PROTO-CORE-API-R71 keeps a time per value and counts a value Core never requested from any turn. Built meanwhile: a time per value; a relaunch reads back until each value is found or the record reaches Core's request for it, and a listing whose tail holds a value but not its turn's start reports it only when Core made no request for it.


## Resolved recently (optional, trimmed in planning passes)
