# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 PROTO-AGENT-EVENTS: `v` is one number "bumped when any event's meaning or shape changes", but the cache must also be invalidated when one adapter's projection logic changes without any shape change. Meanwhile `v` is the schema version (a const in the schema and in `ersa-protocol`), each adapter's `Projector::version()` versions its logic, and Core should key the projection cache on both.
- [ ] 2026-10-31 PROTO-AGENT-EVENTS: R08 names two open payloads, but `tool.requested.input`, `permission.requested.input`, and structured `tool.result.content` carry a tool's own arguments and output and cannot be closed; and a `backend.passthrough` of a line that failed to parse (R10) has no object to carry. Meanwhile the schema leaves those three fields open and allows `payload` to be the verbatim line as a string.
- [ ] 2026-10-31 PROTO-AGENT-EVENTS: `session.state` and `session.ended` are emitted by Core from the lifecycle, and process exit is not a raw line, so re-projecting the raw log alone (R03, ARCH-CORE-R06) cannot reproduce them. Meanwhile the adapter reports exit through `AdapterSink::process_exited`, outside the raw log; the Core ticket decides whether lifecycle facts get raw lines of their own.
- [ ] 2026-10-31 PROTO-AGENT-EVENTS: `question.answered.answers[].value` has no stated shape. Meanwhile it is a string (free text or one option id) or an array of strings (multi-select option ids).

## Resolved recently (optional, trimmed in planning passes)
