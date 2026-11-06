# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 FLOW-ORGANIZE-SESSIONS (Variations and errors), PROTO-CORE-API: a session with no backend session id offers Rename as unavailable, but `session.list` does not say whether a session has one, so the Interface cannot tell. #195 shows Rename unavailable while a session is starting, and otherwise lets `session.set_title` answer `invalid_state`, which keeps the field open with the reason; `session.list` could report whether the backend has named the session.

## Resolved recently (optional, trimmed in planning passes)
