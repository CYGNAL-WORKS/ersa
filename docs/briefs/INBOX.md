# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 PROTO-CORE-API: "when the limit is reached or the send is canceled, the task ends and nothing is left waiting on the file system" can't hold for a read that hangs inside the operating system (a network drive that stops answering): Core answers at the limit and the job stops at its next step, but the one blocking call already under way keeps its thread until it returns. Meanwhile Core bounds the cost: a session's next send waits at most 30 seconds for the stuck copy and is refused rather than queue a thread, and at most 8 `attachment.inspect` reads may be outstanding Core-wide. Proposed wording: "Core answers at the limit, and the work stops at its next step."
- [ ] 2026-11-06 PROTO-CORE-API: "a second `session.send` meanwhile is refused with `invalid_state`" conflicts with requests about one session from one connection running in order, so pipelined messages keep their order. Meanwhile a send from the same connection waits for the earlier send's answer (interrupts and answers behind it do not wait), and a send from another connection is refused with `invalid_state`. Proposed wording: "a second `session.send` from another connection meanwhile is refused with `invalid_state`; one from the same connection waits its turn."

## Resolved recently (optional, trimmed in planning passes)
