# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CORE-API: `branch.list` is gone from Core, the schema and the Interface (#424), `api_version` 6. Retire R18; reword R19 so its default states the rule itself (the local branch `origin/HEAD` points at, else `main`, else `master`, else the checked-out commit: "else the checked-out branch" cannot be told apart from the checked-out commit through `workspace.create`); drop `branch.list` from R92 and from the Connection paragraph's and Workspaces' wording. Meanwhile the four default-branch tests in `crates/ersa-core/tests/workspaces.rs` cite R18 and R19.





## Resolved recently (optional, trimmed in planning passes)
