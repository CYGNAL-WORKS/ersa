# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.` Every list item in this file is an open entry in that format, under Open; the manifest check fails on any other, since the merge gate would not hold for it.

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open


- [ ] 2026-11-27 PROTO-CLAUDE-CODE-STREAM: R86 asks for "a reason in words of its own" for each start the Claude Code adapter could not make and names no words; #669 chose one sentence each, which the note shows after its own: `initialize` refused, "Claude Code refused to connect to Ersa."; the process ended first, "Claude Code quit before it gave Ersa its sign-in page."; no usable `automaticUrl`, "Claude Code didn't give Ersa the address of its sign-in page."; and for two cases R86 doesn't name, the adapter's own 60-second limit run out (never reached through Core, whose 20 seconds come first), "Claude Code didn't respond.", and a command found that the host couldn't run, "Ersa couldn't run Claude Code on this host.". CONTENT.md's Errors rule asks for what failed and then what to do, which one sentence after the note's can't hold, and these live in the adapter, as Core's own refusal words do, not in the string catalog. Meanwhile the words stand as written; a planning pass may name them in the brief, or say where a sign-in's what-to-do goes.
