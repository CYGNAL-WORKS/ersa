# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-26 FLOW-CLEAN-UP: R10 archives a session whose last activity, else its creation, is old enough, but a session with no last activity whose creation nothing remembers (no record and no app-log fact; `session.list` dates it 1970-01-01) reads as decades inactive. Meanwhile the sweep leaves such a session unarchived, as it leaves anything it could not check (#541).
- [ ] 2026-11-26 FLOW-CLEAN-UP: R10 says the sweep archives "as `session.set_archived` does", which stops a live session while holding its turn, and R06 says no request waits for a sweep except at a project's worktrees: while the sweep stops a live idle session nobody is subscribed to, a `session.arrange` of it waits and a send is refused (PROTO-CORE-API-R85), for as long as the stop takes. Meanwhile the sweep archives such a session as Archive does, stop included (#541).
- [ ] 2026-11-26 FLOW-CLEAN-UP: R10 does not say whether a session another process has open (the backend's live-session registry shows it, or cannot say) is archived; Claude Desktop does not archive a session open on screen, and R22's put-away refuses that case. Meanwhile the sweep leaves such a session unarchived (#541).
