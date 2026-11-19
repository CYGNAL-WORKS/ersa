# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-19 PROTO-CORE-API (session.list), FLOW-RESUME-SESSION-R07: `backend_session_id` is "absent until the backend has reported it", but for Claude Code Core chooses the id and stores it at `session.create` (PROTO-CLAUDE-CODE-STREAM-R35), and PROTO-CLAUDE-CODE-STREAM says that id names no record until the first message, so `claude --resume` can't find it before then. Meanwhile Core lists the stored id: from creation for a backend that starts under Core's id, from its `session.started` for one that names the session itself, from the record for one found there. Decide whether a chosen id counts as reported, or only once the first turn's `init` or the record shows it.
- [ ] 2026-11-19 PROTO-CORE-API (Notifications), FLOW-RESUME-SESSION-R07: no notification carries `backend_session_id`, and the Interface rereads `session.list` only for a session it doesn't know, so for a backend that names a session as it starts, Copy session ID stays unavailable until something else rereads the list. No such backend ships today (Claude Code's id is listed from creation). Meanwhile nothing rereads; decide whether `session.tile` carries the id or the Interface rereads.


## Resolved recently (optional, trimmed in planning passes)
