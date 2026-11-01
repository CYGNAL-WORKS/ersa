# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-10-31 PROTO-CLAUDE-CODE-STREAM: `result.usage`'s top-level token counts are summed over every API call in the turn (fixture 11 counts the 40k cached prefix twice), so they overstate the context; the projection takes `context_tokens` from the last entry of `usage.iterations`, falling back to the top-level sum when absent.
- [ ] 2026-10-31 PROTO-AGENT-EVENTS: `usage.updated.turns` does not say whether it counts Ersa turns or the backend's API round trips (`result.num_turns`); the Claude projection sets it to the session's turn count.

## Resolved recently (optional, trimmed in planning passes)
