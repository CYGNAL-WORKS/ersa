# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-24 PROTO-CLAUDE-CODE-STREAM: nobody has recorded whether Claude Code 2.1.316, the version the adapter is tested against, answers `get_settings`. If it ignores the request rather than refusing it, backend detection on that version waits the model probe's whole 60-second limit (R82, R56), and the new-session screen says it is detecting backends for that long; its models are still listed, with no default effort. On 2.1.330 the request was answered in under a second (the whole recorded probe took 978 ms) with `applied.effort: "medium"`, recorded as `2.1.330/00-probe-get-settings`. 2.1.316 is not installed on this machine and the repository has no procedure for fetching a pinned CLI version, so the #587 pull request ships on the 2.1.330 recording, with the limit unchanged and no version gate; whether to record 2.1.316, shorten the wait for `get_settings`, or gate it by version is the planning pass's call.
