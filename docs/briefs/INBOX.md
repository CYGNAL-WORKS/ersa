# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM-R40: R40 takes a live session's permission mode from `system/init`, `system/status` and the `initialize` response, and its model also from `set_model`'s answer, but not the mode from `set_permission_mode`'s answer. That answer (`{"mode":"acceptEdits"}`) comes before the `system/status` line (fixtures 25, 26), so `session.set_permission_mode`, which returns the session as `session.list` lists it once the answer is projected, would return the old mode. #335's projector also reads the mode from the answer to the host's own `set_permission_mode`, as it reads the model from `set_model`'s; R40 wants "and from `set_permission_mode`'s answer" added for the mode.
- [ ] 2026-11-14 PROTO-CORE-API-R82: R82 checks an effort against the efforts `backend.list` lists "for the session's model", and does not say what that model is when `session.list` reports one `backend.list` does not list (a model chosen in the backend's own tools, or one the backend dropped). #335 matches the session's model to a `backend.list` model by `id`, else by `resolved` (a live Claude Code session reports the resolved id), else, for a model left to the backend, the one marked `default`; a matched model with no `efforts` takes none, and a model matching none is checked against every effort the backend offers (the adapter's list, as `session.create` checks), so the user can still change its effort.



## Resolved recently (optional, trimmed in planning passes)
