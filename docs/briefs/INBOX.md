# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 FLOW-RESUME-SESSION: Built so far still says a resumed session starts with the values Core last gave it and that the record's newer values (PROTO-CORE-API-R71) are still to build; #367 builds them: a relaunch now weighs the record's latest against the first turn after each request. Left as it is for the planning pass.
- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM-R60: it does not say what "the first turn that began after that time ran with" is for the model and effort when that turn was interrupted before any reply. Built: the earliest reply whose turn began after the time, so such a turn gives its permission mode but the model and effort come from the next turn that has a reply; the other reading (none) would make Core pass the record's latest and lose an alias.
- [ ] 2026-11-14 PROTO-CORE-API-R71: the relaunch sentence compares the record's latest with the first turn after the request; for a value that went A, then B, then back to A, "once a later turn ran with another" could mean the change already happened. Built: the comparison is of the latest value with the first's, so A-B-A passes Core's request.
- [ ] 2026-11-14 PROTO-CORE-API-R71: the relaunch sentence covers only a value Core gave a time for. For a value with none (left to the backend at `session.create`, or stored before Core kept times, R97) there is no first turn to compare with. Built: the relaunch passes what Core stored, nothing for a value left out, so the backend's default stays one (R71's first clause); the cost is that a change the user made in the backend's own tools to a value left out is lost on relaunch, though `session.list` shows it. Pinning the record's latest instead (as Invocation's "when the session reports them" reads) would fix that but freeze the default model and effort; the planning pass should choose.
- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM: Invocation says `--model` and `--effort` are passed "on a relaunch when the session reports them", which reads as `session.list`'s values; R71's relaunch sentence passes Core's request instead while the record's latest is what the first turn after it ran with (an alias stays an alias). Built per R71; Invocation should say so.



## Resolved recently (optional, trimmed in planning passes)
