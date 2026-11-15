# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 FLOW-READ-TRANSCRIPT: Built so far still says the Interface takes a target for a path when it lies inside the workspace folder or is spelled as an absolute path; since #368 `tool.requested` carries `target_kind` (PROTO-AGENT-EVENTS-R26) and the Interface makes only a `path` relative and cuts it at its start, so that sentence can go. Built meanwhile to R08 and TRANSCRIPT.md, Tool cards.



## Resolved recently (optional, trimmed in planning passes)
