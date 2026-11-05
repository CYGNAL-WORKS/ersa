# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-05 LAY-SHELL: the foot's reasons now call Core "Ersa's background process" when it cannot be started, stops before it is ready, or is not ready in time; the glossary has no user-facing term for Core, so this phrase is in use without one. Used as is in the Shell's launch errors.
- [ ] 2026-11-05 LAY-SHELL: after Ersa reaches the sessions again, "Reconnected" stays in the foot's hidden status region until the next problem replaces it, so the region is not empty while all is well and a screen reader user browsing the rail can come across it; clearing it needs a timer or another trigger the brief does not name. Left in place.
- [ ] 2026-11-05 FLOW-CLOSE-AND-QUIT: the quit question shows the Shell's quit errors after "Ersa did not quit:", and those still name Core ("cannot find the Core serving …", "cannot end Core (process …)", "Core (process …) is still running after it was ended"); the foot's rewording did not reach them. Left as is.

## Resolved recently (optional, trimmed in planning passes)
