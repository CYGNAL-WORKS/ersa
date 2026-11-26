# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-26 PROTO-CORE-API: Signing in does not say whether a sign-in is "under way" (for R158's `signing_in`, R159's notification and R160's cancel) while its adapter is still starting it, before `backend.sign_in` has answered. If it is, a start that is then refused or runs out its 20 seconds leaves other windows showing a sign-in that nothing says has ended (R157 sends no `backend.sign_in_ended`), and FLOW-SIGN-IN-R09 reads the listing again only when one ends. Built meanwhile (#635): only the requests waiting on the start know of it; `signing_in` appears, and an ending is sent, only once the start has answered; a `backend.cancel_sign_in` during the start stops it (admission decides a start the cancel overtook, which never counts), the waiting `backend.sign_in` answers `invalid_state` "The sign-in to <label> was canceled.", and nothing is sent. FLOW-SIGN-IN-R14 says a `canceled` ending adds no words, so a window whose start another window canceled shows that message as a sign-in that couldn't start.
- [ ] 2026-11-26 PROTO-CORE-API: Signing in names `core.shutdown` as one way a sign-in ends, but `backend.sign_in_ended`'s outcomes (signed_in, failed, canceled, timed_out) have none for it. Built meanwhile (#635): it ends `canceled`, as a cancel does. In practice no window receives it, since every connection closes on shutdown before the sign-in has stopped.
