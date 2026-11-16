# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: R78 says the chain follows `logicalParentUuid` across a compaction, but a boundary that kept a segment names the segment's tail as its logical parent, so after an edit inside the kept segment following it brings the edited-away entry back, against R78's own "no entry of an abandoned branch". R78 should say a boundary with `preservedSegment` does not follow its `logicalParentUuid` (the history before it comes through the segment's head). Built to that meanwhile (#435).





## Resolved recently (optional, trimmed in planning passes)
