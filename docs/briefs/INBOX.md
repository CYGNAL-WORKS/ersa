# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-07 PROTO-CORE-API: `session.delete` step 2 asks the live-session registry before stopping the session, but Core cannot tell its own CLI process from another in the registry (no transport reports a child's pid), so a session Core runs would always look open. #197 stops the session first, then asks; a CLI leaves the registry as it exits. The one difference: a session Core runs that another process also has open is stopped before the refusal. The brief should order the stop first, or say how Core tells its own process.
- [ ] 2026-11-07 PROTO-CORE-API: `session.delete` refuses a stale token with "`detail` the new `workspace.changes` object", but an error's `detail` is a string in the schema. #197 puts the object there as JSON text. The brief should say so, or the schema should let `detail` be an object.
- [ ] 2026-11-07 PROTO-CORE-API: `session.delete` is silent on a call without `token` for an isolated workspace. #197 answers `invalid_params` before stopping or claiming anything.
- [ ] 2026-11-07 PROTO-CORE-API: step 7 removes "the workspace when deleted", but a workspace whose files a delete took can still hold another session marked deleting (one whose own delete keeps the files, since `workspace.changes` leaves it out of `sessions`). #197 keeps the workspace's row while any session refers to it, reports `workspace_deleted` for the delete that took the folder, and has the last session out remove the row when the folder is gone. The brief should say what becomes of that row and what `workspace_deleted` means then.

## Resolved recently (optional, trimmed in planning passes)
