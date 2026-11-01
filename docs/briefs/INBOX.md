# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 PROTO-CLAUDE-CODE-STREAM: the SDK 0.3.316 types lack `system/task_summary`, `system/post_turn_summary`, and the `initialize` response's `capabilities`, `pid`, `current_permission_mode` and `session_state`, all emitted by 2.1.302; the CLI binary's own protocol schemas mark the two system subtypes `@internal`, so the brief's rail-activity derivation leans on unsupported surface. The wire models follow the fixtures.
- [ ] 2026-10-31 PROTO-CLAUDE-CODE-STREAM: the brief shows no stdin user-message shape; the SDK types mark `parent_tool_use_id` required, but every recorded user message omits it and the CLI accepts that, so the wire model omits it when unset.
- [ ] 2026-10-31 PROTO-CLAUDE-CODE-STREAM: Versioning says `claude_code_version` is read from the `initialize` response while the system/init section says from `system/init` (both carry it), and the `initialize` response also carries its own, shorter `capabilities` list (2 entries against init's 11); the wire layer models both and the capabilities helper reads `system/init` per R06.

## Resolved recently (optional, trimmed in planning passes)
