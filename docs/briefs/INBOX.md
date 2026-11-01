# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-01 PROTO-CORE-API: a view holds at most 64 MiB of events (merge review of #35, item 1), so `session.subscribe` also returns `first_seq`, the first event the view still holds, when it no longer holds every event from 0; a subscription from before it starts there, and the Interface starts over from `first_seq` rather than 0 (with a notice that earlier events are no longer held). The brief's Sessions section says only "subscribes again from 0".

## Resolved recently (optional, trimmed in planning passes)
