# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-13 PROTO-CLAUDE-CODE-STREAM: the 2.1.316 recordings for Tool targets and edits (fixtures 28 to 34) agree with the table's fields, with two corrections to its lead-in: `system/init` lists `Task`, but the call arrives named `Agent` (34-tool-agent), and `NotebookEdit` and `WebSearch` are deferred tools, loaded first through a `ToolSearch` call that the table does not name, so it carries neither. The adapter matches both `Task` and `Agent` meanwhile.
- [ ] 2026-11-13 PROTO-CLAUDE-CODE-STREAM: 2.1.316 offers `PowerShell` (a `command`, like `Bash`), which the Tool targets and edits table does not name, so its calls carry no `target` under R48 and their cards show no command; the adapter follows the table meanwhile. Should `PowerShell` join `Bash`'s row?
- [ ] 2026-11-13 FLOW-READ-TRANSCRIPT: an `Edit` call with `replace_all` replaces every occurrence, but its `edits` entry (PROTO-AGENT-EVENTS-R20) carries only the old and new text, so a diff card cannot say it changed more than one place. The adapter projects it as one entry meanwhile; whether the card says "every occurrence" (and so whether the edit carries it) is open.

## Resolved recently (optional, trimmed in planning passes)
