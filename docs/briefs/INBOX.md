# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-01 PROTO-CORE-API: owner direction (#35): a session's title and creation time are its backend record's, its mode is Ersa's. Built: the session row keeps host, workspace, backend, backend session id and mode; `title` and `created_at` are read from the record (custom-title, ai-title, last prompt; first entry's time, else the app log's first fact); `session.set_title` and a `title` on `session.create` answer `invalid_state`. The brief still offers both; decide whether to drop them or rename through the backend (Claude Code `/rename`).
- [ ] 2026-11-01 ARCH-CORE: owner asked (#35) whether Core needs a database at all once a session row only points at the backend's session: what remains is projects, workspaces and their state machine, settings, and those pointers. Kept SQLite; decide before #40 adds trust to projects.
- [ ] 2026-11-01 PROTO-CORE-API: `session.reproject` is withdrawn but still in the schema, answering `invalid_state`, since removing a method bumps `api_version` (Versioning). Decide whether to remove it with a bump.
- [ ] 2026-11-01 PROTO-AGENT-EVENTS: "each time a session is opened" is not defined. Built: a session Core runs is opened when its backend starts and never re-projected while live; one it does not run is opened on its first subscribe, and afresh on a `from_seq: 0` subscribe while nobody else is subscribed (so turns taken elsewhere show); a resubscribe from later keeps its view. A client resubscribing from later after a Core restart gets a new projection it cannot tell from the old: `session.subscribe` may need a projection identity.
- [ ] 2026-11-01 ARCH-CORE: the app log is read back at startup, not only written: a session whose last fact is `process_started` gets `core_restarted`; any other last fact is shown again at the end of a reopened view; the first fact's time stands in for a creation time the record cannot give. The brief describes the app log as observability only.
- [ ] 2026-11-01 PROTO-AGENT-EVENTS-R12: with resume out of scope, Core opens a live view only before its backend starts, so a message in both record and live stream cannot yet arise end to end; the merge is verified on Core's projection with a stub projector and in the Claude adapter's message ids. Two consequences the brief should settle: a live line whose message the record already had yields no event, against R02 ("every raw log line shall project to at least one event"); and streamed partial `assistant.text` events carry no message id, so a message still streaming when its record entry was read would show partials whose final is dropped. When resume lands, the user's prompt is live only as Ersa's host line (no message id) while the record has it under a `uuid`, so it would show twice unless the merge also matches prompts.

## Resolved recently (optional, trimmed in planning passes)
