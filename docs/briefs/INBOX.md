# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 PROTO-AGENT-EVENTS: R14 is untestable as written, since Core has no resume yet (`session.resume` and `session.start` on a stopped session answer `invalid_state` naming M1). #54 built the adapter's open-elsewhere answer, the notice step Core's resume is to call before starting the backend, and that step before a record rename (#54's scope addition); R14 stays unverified until the resume exists, and the rename's notice has no requirement ID to cite.
- [ ] 2026-11-02 PROTO-AGENT-EVENTS: a Core-made `session.notice` is in no record, so it reaches only the clients reading the session when it is emitted and is gone from the next view; #54 publishes it to the open view and logs it when none is open.

## Resolved recently (optional, trimmed in planning passes)
