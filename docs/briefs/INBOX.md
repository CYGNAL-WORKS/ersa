# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-18 FLOW-ORGANIZE-SESSIONS: R04 shows every section "empty or not", then "No section" for sessions in none, but doesn't say whether "No section" shows while it holds no session. #326 shows it only while it holds a session shown, as date groups do, so an empty heading with no actions never sits at the end.
- [ ] 2026-11-18 none (docs/design/RAIL.md, Menus): RAIL.md says where focus goes when a tile leaves, not when a section heading does. #326 moves focus, once a section deleted from its heading's menu has gone, to the heading after it as drawn before, else the one before, else New session, as a leaving tile's does.
- [ ] 2026-11-18 LAY-SHELL: Loading, empty and failed names projects, workspaces and sessions that can't be read, not sections. #326, grouped by section with the sections unreadable, says "Ersa can't read your sections." with the reason and puts every session under no heading, as when the projects can't be read; while they are first read, the tree shows Loading.


## Resolved recently (optional, trimmed in planning passes)
