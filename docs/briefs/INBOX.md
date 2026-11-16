# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CLAUDE-CODE-STREAM: R20 names only `logicalParentUuid` across a compaction, but a compaction that keeps a segment (the `compact_boundary`'s `compactMetadata.preservedSegment`, fixtures 2.1.316/44 and 45) leaves the entries after it chained to the segment's tail, so only `preservedSegment.anchorUuid` names the summary. R20 should say the chain also runs from a kept segment's head (`headUuid`) to that summary (`anchorUuid`), and that entries project in record order, so the summary shows once after the kept segment. Built to that meanwhile (#430).





## Resolved recently (optional, trimmed in planning passes)
