# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 PROTO-CLAUDE-CODE-STREAM: R21 says "the same events", but the stream's synthetic message numbers its block under its own `message.id`, which the record's `system/local_command` entry does not carry; the record numbers it under the entry's `uuid`, and the test compares with block ids taken from the uuid. Also, a `local_command` entry may be the command's echo rather than its output (2.1.215 wrote both); only content wrapped in `<local-command-stdout>`/`<local-command-stderr>` is projected, unwrapped.

## Resolved recently (optional, trimmed in planning passes)
