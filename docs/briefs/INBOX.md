# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 PROTO-CORE-API: a Windows pipe name has no per-process override (Unix has `XDG_RUNTIME_DIR`), so the test of a Core on an explicit endpoint beside one on the default stands in a second labelled endpoint for the default on Windows, rather than touch the user's own pipe; say whether that suffices or whether Core should honour an endpoint override from the environment.

## Resolved recently (optional, trimmed in planning passes)
