# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-24 FLOW-CLEAN-UP: step 2 says the composer's status line says the folder is being put back when the user resumes the session as well as when they write to it, but only a send reports progress (R05, PROTO-CORE-API-R134); a Resume from the archived note gets no `session.send_progress`, so the note reads "Resuming…" for the whole put-back, up to 10 minutes. Meanwhile #538 shows the words only for a send, and the note keeps "Resuming…"; decide whether a resume reports progress or the step is reworded.
- [ ] 2026-11-24 FLOW-CLEAN-UP: #538's Done when asks for a live check against a Core with a put-away workspace, but nothing in the running Core puts a workspace away yet (no sweep, and archiving removes no folder; that is #539), so the real window can't reach one. Meanwhile #538 checks the words over the fake Core and in the gallery and adds no live check; move the live check to #539's Done when.
