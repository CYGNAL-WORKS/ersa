# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-CORE-API: `session.send` does not name a refusal while `session.set_archived { archived: true }` is between its busy check and its mark. Without one, a send from another connection could start after the check and be stopped by the archive, against R77's "change nothing". Meanwhile Core refuses such a send with `invalid_state` and no `reason`, saying the session is being archived, as it refuses a send to a session being forked (#313).

## Resolved recently (optional, trimmed in planning passes)
