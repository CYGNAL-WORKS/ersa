# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-13 PROTO-AGENT-EVENTS: the open question on record and live input is answered by fixtures 2.1.316/28-resume-original and 29-resume-while-open: a resumed CLI repeats none of the record on its stream, so a resumed view is the record's events then the live ones, with R12's de-dup as the only guard and no exception to R02 needed. But Sequencing ("`session.started` first … `session.ended` last" within a session) and Opening ("projected when its backend starts") don't fit: Core keeps the view a client is reading across the resume, numbering on past the earlier `session.ended`, so a resumed run's `session.state`, `session.started` and its own `session.ended` follow it. Built that way meanwhile (#321).
- [ ] 2026-11-13 PROTO-CORE-API: `session.interrupt` during a send's `starting` phase on a session the send resumed cancels the message (PROTO-CORE-API-R25) but leaves the resumed backend running, idle; the brief doesn't say which. Built that way meanwhile (#321).
- [ ] 2026-11-13 FLOW-FORK-SESSION: resuming a fork that has no record of its own forks its original again (PROTO-CLAUDE-CODE-STREAM, Invocation), which copies the original's conversation as it is now: turns taken after the fork point, or a turn part way (fixture 18), against FLOW-FORK-SESSION-R01. `session.fork`'s refusals (finished reply, open elsewhere) are not repeated. Built without them meanwhile (#321).
- [ ] 2026-11-13 PROTO-AGENT-EVENTS: Core's lifecycle facts (Projection rules) name a fork's process ready but not a resumed one's. A resumed CLI reports nothing until its next message, so Core writes a `resumed` fact to the app log once the resumed backend is ready, which projects as `session.state` idle from `starting`, as a fork's does. Built that way meanwhile (#321).
- [ ] 2026-11-13 FLOW-RESUME-SESSION-R01: a resumed session is started with the model, effort and permission mode Core stored, not as PROTO-CORE-API-R71's relaunch clause says (the record's latest value once a later turn ran with another; PROTO-CLAUDE-CODE-STREAM, Invocation). R71 is not built yet, so a session whose model was changed in the backend's own tools resumes on Core's older choice. Built with Core's stored values meanwhile (#321); R71's ticket should cover the relaunch.

## Resolved recently (optional, trimmed in planning passes)
