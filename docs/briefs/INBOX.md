# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-25 FLOW-ARCHIVE-DELETE-SESSION: R05 makes Delete unavailable while the session is starting, working or waiting on the user, but not while a message is being sent to it, though Core refuses `session.delete` with `session_busy` then (PROTO-CORE-API, Errors). #483 offers Delete unavailable during a send too, with R05's reason ("Stop the session to delete it"), as Archive (R11) and Fork already are, and its tests cite R05.
- [ ] 2026-11-25 none (docs/design/COMPOSER.md, The status line): the table has no row for the new-session screen finding whether a picked folder is a project or adding it after Trust, though FLOW-NEW-SESSION-R42 holds send then. #483 shows the ring at `size-ring` and "Checking the folder…" or "Adding the project…" in the status line's place, announced as a status, after "Ersa can't reach your sessions" and before every other line.
- [ ] 2026-11-25 LAY-SHELL: Open questions still asks whether the foot shows that Ersa is trying after Retry is pressed. #483 answers yes: until the try attaches or fails, the foot shows the trying-again line with its ring, without a reason, as it does while a lost connection reattaches; the first connect stays silent.

