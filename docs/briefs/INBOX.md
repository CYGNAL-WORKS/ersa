# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-18 none (RAIL.md, Menus): a menu opened at the pointer has no rule for the window's edge. Built (#290): it opens the other way from the press where there is no room, is held inside the window with no inset, and is placed again when it or the window changes size; a menu larger than the window keeps its top-left corner in. Needs a rule, and an ID the tests can cite.

## Resolved recently (optional, trimmed in planning passes)
