# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: the CLI binary's embedded protocol schemas (the source of the SDK `.d.ts`) mark `system/task_summary` and `system/post_turn_summary` `@internal`, which is why the SDK omits them; the brief still derives rail activity from them, so that state rests on unsupported surface. Nothing changed meanwhile; the wire layer parses them and would pass anything unknown through.

## Resolved recently (optional, trimmed in planning passes)
