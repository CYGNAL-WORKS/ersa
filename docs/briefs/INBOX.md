# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-12 PROTO-CORE-API (`session.set_title`): main's rewrite for PROTO-CORE-API-R70 says `session.set_title` answers `invalid_state` for a session whose backend has not reported its id yet, "which only a fork before its first message is", and names only a new session's id as known from its start. On #272 a fork's id is chosen and stored before its CLI starts (`--session-id`, PROTO-CLAUDE-CODE-STREAM-R38), so a fork can be renamed before its first message, as the 2026-11-08 change from #272's inbox says. Meanwhile the code renames it. Decide which the brief says.
## Resolved recently (optional, trimmed in planning passes)
