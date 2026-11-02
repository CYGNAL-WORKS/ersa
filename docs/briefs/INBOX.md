# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 PROTO-CORE-API: the root workspace's `vcs_ref` is not stated; Core reports it empty (the root's checkout is the user's to change), and #52 did not read the branch at listing time.
- [ ] 2026-11-02 PROTO-CORE-API: `backend.list` replaced `min_tested_version`/`max_tested_version` with `tested { min, max }`, a removal that Versioning says bumps `api_version`; #52 kept version 2, since no client read those fields. Planning to confirm or bump.
- [ ] 2026-11-02 PROTO-CLAUDE-CODE-STREAM: Invocation says the CLI's own default model is marked but names neither it nor the default effort; the adapter marks `opus` and `medium`, what 2.1.316 sessions started without `--model` or `--effort` ran with (its session-record fixtures). The CLI's real default may also come from the user's settings or `ANTHROPIC_MODEL`, and then `session.list` reports a model the session did not start with (R15). Core stores only what was named, so a later start leaves the rest to the CLI. Planning to choose: always pass the default, or report the backend's own announcement.
- [ ] 2026-11-02 PROTO-CORE-API: a replayed session reports empty `model`, `effort` and `permission_mode` (the recording does not carry the ids it started with); sessions created before #52 report their backend's defaults.

## Resolved recently (optional, trimmed in planning passes)
