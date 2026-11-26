# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-26 FLOW-SIGN-IN: under a session R17 removes the note as soon as a sign-in for the session's backend ends `signed_in`, yet R17 also says the note behaves as R12 says, and R12's words ("The sign-in finished, but Claude Code still reports that it isn't signed in.") only follow a `signed_in` end, so under a session they can never show: a person whose sign-in "worked" but whose backend still says signed out loses the note and finds out only when the next message fails. Built as R17 reads (the note goes on any `signed_in` end). The alternative is to clear it only when the listing read after that end no longer reports the backend `signed_out`, and otherwise show R12's words.
