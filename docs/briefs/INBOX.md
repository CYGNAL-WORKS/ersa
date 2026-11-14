# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-14 PROTO-CLAUDE-CODE-STREAM: the model probe's time limit is not named; #322 uses the session handshake's (60 s), and the probe starts as a new session (`--session-id`, user settings only, no project, Core's own folder) because that is the case `initialize-only-2026-11-09.txt` recorded as writing no record. A trusted project's settings could narrow the list (`availableModels`) and the probe would not see it.
- [ ] 2026-11-14 PROTO-CORE-API: whether Core reuses `backend.list`'s models is not named; #322 keeps them with the version check's result (for Core's life when every backend is available, 30 s after a failure, as detection already did), so a change of plan or sign-in shows after Core restarts. A probe that fails after the version check is `error` with its version still listed; the brief's "version (optional, present when available)" could be read as dropping it.
- [ ] 2026-11-14 PROTO-CORE-API: `session.create` refuses a named model when `backend.list` lists none for the backend (missing, or unable to report), as R72 offers none; #322 checks the model against the host's detected list, so detection, including the model probe, now runs before a session starts (cached after the first).


## Resolved recently (optional, trimmed in planning passes)
