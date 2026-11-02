# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-02 PROTO-CORE-API: no brief describes the replay capture format; #64 added a top-level `core` field (the commit a capture was written from, `-dirty` when uncommitted) that the comparison ignores. The glossary's **Core** is the process, not a commit, so the name may be wrong. Left as `core` until a planning pass names it.

## Resolved recently (optional, trimmed in planning passes)
