# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CORE-API: R105 and R106 are built (#414), so the `session.tile` sentence "Until R105 and R106 are built, a corrected date created reaches a window at its next read of the list" no longer holds; meanwhile the code follows R105 and R106.
- [ ] 2026-11-15 PROTO-CORE-API: the tile's `created_at` is a required field in the schema (the brief gives it no "absent while"), and `api_version` stays, reading Versioning's "new optional fields" as covering a field only Core sends; the Changes line for R105 does not say, so the call is recorded here.




## Resolved recently (optional, trimmed in planning passes)
