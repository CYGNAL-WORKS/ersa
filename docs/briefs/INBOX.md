# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-15 PROTO-CORE-API: `workspace.remove` and `project.remove` are refused only "while the workspace has sessions Ersa started", and FLOW-RESUME-SESSION-R09 says sessions started in the backend's own tools don't hold removal back, but neither says what happens when the user has resumed such a session in Ersa and Core is running it; meanwhile Core refuses with `invalid_state` until it is stopped (no backend is left running in a removed folder), and `workspace.changes` counts it in `sessions` while it runs.
- [ ] 2026-11-15 FLOW-ARCHIVE-DELETE-SESSION: the brief is silent on sessions started in the backend's own tools in an isolated workspace when the user deletes its last session Ersa started; meanwhile `workspace.changes` doesn't count them (unless Core is running one), so the delete takes the workspace's files, and Core forgets them with the workspace, their records left, as R09 does for `workspace.remove`.
- [ ] 2026-11-15 PROTO-CORE-API: `session.remove` "removes the session alone", but once sessions are found in the backend's records, a session Ersa started that is removed while its record exists shows again on the next `session.list` as one started elsewhere, under a new id; meanwhile Core lets it, since ADR 0033 lists what the record holds.
- [ ] 2026-11-15 FLOW-RESUME-SESSION: R04 says every session the backend's record holds, while PROTO-CLAUDE-CODE-STREAM notes Claude Code's own list hides sessions whose entrypoint is `sdk-cli`, `sdk-ts` or `sdk-py`; meanwhile Core lists them all, as R04 reads, so sessions other SDK tools ran in the folder show too.

## Resolved recently (optional, trimmed in planning passes)
