# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-06 RAIL.md (A project heading): at size-rail a collapsed, untrusted heading cannot hold its name, "Not trusted", "N sessions" and the kept place of New session and More: the name is cut to a letter. #158 shows the count's number alone beside "Not trusted", with the full "N sessions" heard by assistive technology; a trusted collapsed heading shows "N sessions".
- [ ] 2026-11-06 LAY-SHELL: Built so far does not yet say that New session sits at the rail's top and that group headings collapse, counting their sessions with the mark in the tone of what they want; #158 builds both, with collapsed headings kept while the window is open (across restarts arrives with the arrange menu).

## Resolved recently (optional, trimmed in planning passes)
