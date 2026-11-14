# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-13 none (ADR 0035): the ADR doesn't say where the daily build installs. Tauri's per-user installer uses `%LOCALAPPDATA%\Ersa`, which is also Ersa's application data folder, where a development build keeps its data (ersa.db, core.log, worktrees) and the daily build's `daily` folder sits. No file names collide, and the uninstaller removes only the files it installed, so the data stays; #318 keeps Tauri's default meanwhile. Both builds also share the webview's data folder `%LOCALAPPDATA%\dev.ersa.app`, since they share the bundle identifier.

## Resolved recently (optional, trimmed in planning passes)
