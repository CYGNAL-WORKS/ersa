# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 FLOW-NEW-SESSION-R02: R02 says the session controls start at the values last used for a project, but not what happens to a model, effort or permission mode the user already chose on the screen when the project then changes (chosen from the project chip, added, or found, as in R19's first run). Choosing a project replaced them with its last-used values, so the session started at the defaults (#397). Built meanwhile: the how values chosen on the screen stay, with the backend they were chosen for, and the rest come from the project's last-used values.
- [ ] 2026-11-14 PROTO-CORE-API-R71: R71 has `session.list` report a model as the backend's newest report, so a session started with an alias or Default lists the id it resolved to (`opus` becomes `claude-opus-5-5`) once a turn has run, and the session controls named it by its raw id, or by another choice that resolves to the same id (#397). Built meanwhile: Core lists a model the backend reports by the id that Core's own request resolves to, as `backend.list` gives it, as that request; any other model is listed as reported, and the session controls name such a model by the choice listed under that id, else one not marked default whose `resolved` is that id, else the default; a model the host no longer lists takes every effort, as the code already said of a model the backend does not list.



## Resolved recently (optional, trimmed in planning passes)
