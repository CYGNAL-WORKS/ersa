# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 none (docs/design/RAIL.md, DESIGN.md Iconography): RAIL.md says every backend shows one neutral icon until the vendors' terms are checked, and DESIGN.md says icons are stroked Lucide; per the owner's 2026-11-15 decision in #399, a Claude Code tile now draws the Claude spark and a Copilot tile Copilot's icon as filled ink-toned glyphs, any other backend a Lucide terminal. Built to the issue meanwhile.




## Resolved recently (optional, trimmed in planning passes)
