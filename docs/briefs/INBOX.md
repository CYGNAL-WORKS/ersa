# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-04 PROTO-AGENT-EVENTS: R17 says each message id counts "at the latest count reported for it" and that the sum "never decreases", which conflict when an `assistant` line carries an earlier, lower count than a `message_delta` already gave for that id (2.1.316 sends the assistant line mid-stream with a partial count). Built as the highest count seen per id; R17 could say "highest" instead of "latest". Also unstated: synthetic `assistant` messages (`model: "<synthetic>"`, no API call) are left out of the count.

## Resolved recently (optional, trimmed in planning passes)
