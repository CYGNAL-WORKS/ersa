# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-13 FLOW-CLOSE-AND-QUIT-R09: R09 doesn't say where the daily build's web data goes when Ersa's application data folder can't be found (LOCALAPPDATA unset), when there is no `daily` folder and the window only says why it has no Core. The webview's default folder is not an option, since it comes from the system rather than LOCALAPPDATA and is the development build's. Built meanwhile (#346): the folder `ersa-daily-web` in the system's temporary folder. To decide: whether that is right, or the daily build should refuse to open its window instead.

## Resolved recently (optional, trimmed in planning passes)
