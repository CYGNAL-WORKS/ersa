# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 PROTO-CORE-API: `workspace.changes` counts every session in the workspace, because the `deleting` mark it should leave out (Workspaces: `sessions`) arrives with #197; a `TODO(#197)` marks the count meanwhile. #197's Done when has no box for adding the filter and its test, so a planning pass should add one there, then delete this line.

## Resolved recently (optional, trimmed in planning passes)
