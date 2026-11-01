# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 PROTO-CORE-API: the brief says only that the replay server accepts Core's requests; M0-T7 also refuses `project.add/remove`, `workspace.create/remove` and `settings.set` with `invalid_state` (a replay must not create worktrees in the user's repositories), keeps the replayed session open and idle after its last line until `core.shutdown`, and defaults `--pace` to `realtime`. Confirm or state otherwise.

## Resolved recently (optional, trimmed in planning passes)
