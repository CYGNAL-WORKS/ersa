# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 FLOW-NEW-SESSION-R02: R02 says the session controls start at the values last used for a project, but not what happens to a model, effort or permission mode the user already chose on the screen when the project then changes (chosen from the project chip, added, or found, as in R19's first run). Choosing a project replaced them with its last-used values, so the session started at the defaults (#397). Built meanwhile: the how values chosen on the screen stay, with the backend they were chosen for, and the rest come from the project's last-used values.
- [ ] 2026-11-14 PROTO-CORE-API (`backend.list`, `resolved`): `resolved` is there "so the session controls can name" a reported model, but nothing says which choice names it when several resolve to the same id, as Default and an alias can, and `session.list` carries no record of which one the session was started with. Built meanwhile: the choice listed under that id, else one not marked default that resolves to it, else the default; a session started with Default whose model an alias also resolves to shows that alias after its first turn. A model the host no longer lists takes every effort, as the code already said of a model the backend does not list.



## Resolved recently (optional, trimmed in planning passes)
