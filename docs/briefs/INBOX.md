# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 PROTO-CORE-API: `backends[].status` values are not listed; M0-T5 uses `available | missing | error`, with `version` optional (absent unless available).
- [ ] 2026-10-31 PROTO-CORE-API: Errors says every error has `data.kind`, but no kind fits parse error, invalid request, method not found, or invalid params; M0-T5 sends `data` only on `-32000` application errors (schema requires it there) and invalid params use `-32602`.
- [ ] 2026-10-31 PROTO-CORE-API: no error kind fits a `core.hello` version mismatch; M0-T5 uses `invalid_state` with the supported versions in the message and `detail`.
- [ ] 2026-10-31 PROTO-CORE-API: `settings.get` does not say whether defaults are included; M0-T5 returns only what the user set (default `workspaces.root` is applied by Core, not shown). `workspaces.root` is the setting's name.
- [ ] 2026-10-31 PROTO-CORE-API: `session.list.last_event_seq` and `session.subscribe.last_seq` have no value before a session's first event; M0-T5's schema makes both optional. Also: what `workspace.remove` returns for the removed object's `state` (M0-T5: `removing`), and whether `project.remove` may cascade (M0-T5 refuses while workspaces exist).

## Resolved recently (optional, trimmed in planning passes)
