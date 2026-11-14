# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-13 PROTO-CLAUDE-CODE-STREAM: Bare mode's signal misreads a signed-out user who has auto-memory off. Claude Code 2.1.316 run as Ersa runs it, with an empty `CLAUDE_CONFIG_DIR` and `--settings '{"autoMemoryEnabled":false}'` (2026-11-13, not kept), wrote a `system/init` with neither `scratchpad_path` nor `memory_paths` and then failed with `authentication_failed`, so R51 tells that user Claude Code started in its minimal mode instead of asking them to sign in. Fields that differed from the bare fixture: `messaging_socket_path` was present (absent in 2.1.316/28-bare-mode, present in 06) and 28 tools were listed (4 in bare). Meanwhile the adapter implements R51 as written.
- [ ] 2026-11-13 PROTO-CLAUDE-CODE-STREAM: Bare mode cites the summary `2.1.316/bare-mode-2026-11-13.txt`, which #148 replaced with the numbered fixture `2.1.316/28-bare-mode` (stream, record, and meta with `--bare` in its arguments) and deleted. Meanwhile the brief's citation points at a missing file.

## Resolved recently (optional, trimmed in planning passes)
