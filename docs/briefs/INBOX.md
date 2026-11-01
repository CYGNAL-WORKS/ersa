# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: stream and session-record fixtures recorded on 2.1.316 with Ersa's new invocation (`2.1.316/00-session-tools`, `01-session-resume`, records under `2.1.316/record/`, recorder `scripts/record-claude-session.mjs`); the adapter's maximum tested version is now 2.1.316. Versioning still says 2.1.302 for both ends, and the opening paragraph names only the 2.1.302 fixtures.
- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: Session record says a record entry's `message` is identical to the stream message; with `--include-partial-messages` the stream's `assistant` message has `stop_reason: null` and partial `usage` where the record has the final values. Content blocks are identical, and the projection reads neither field, so R15 holds; the wording should say content.
- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: session-record fixtures keep `user`, `assistant`, `custom-title`, `ai-title` and `last-prompt` entries verbatim and cut every other entry to its kind (`"ersa_elided": true`), because those bodies carry the CLI's system prompt and the machine's configuration. Conformance should say whether that is the intended evidence.
- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: the encoded cwd has two rules the brief omits (from the Agent SDK's reader, not yet recorded): characters count in UTF-16 units, so an emoji becomes two dashes, and a name longer than 200 characters is cut to 200 and given `-<base-36 hash of the path>`. Both are implemented.
- [ ] 2026-11-01 PROTO-CLAUDE-CODE-STREAM: Listing's "last prompt" is the CLI's `last-prompt` entry (`lastPrompt`), else the last prompt entry in the tail; the brief could name the entry. Print-mode sessions never get `custom-title` or `ai-title` (`/rename` sent as a message reaches the model as text), so Ersa's own sessions list under their last prompt unless renamed elsewhere.
- [ ] 2026-11-01 PROTO-AGENT-EVENTS: record `user` entries project to `user.message` with origin `human`, or `system` when the CLI wrote them (`isMeta`, `isCompactSummary`). The wrapper also has `origin.kind` (`human`, `peer`, `task-notification`), unused for now; Projection could say whether a record prompt's origin comes from it.
- [ ] 2026-11-01 ARCH-CORE: the adapter reads the session record and the live-session registry from local files; the transport interface has no file access, so a remote host's record cannot be read yet.

## Resolved recently (optional, trimmed in planning passes)
