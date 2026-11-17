# Inbox

Findings from implementing sessions that need a planning pass: a brief, `CONTENT.md` or `DESIGN.md` that contradicts what the branch's code relies on, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session. A harness or process proposal no code depends on is a follow-up in the pull request, not an entry.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The merge gate holds a pull request while its inbox has any `- [ ]` line (ADR 0025); CI stays green.

## Open

- [ ] 2026-11-16 ARCH-CORE-R18: "every input to every reachable state of a small world" does not say whether one world must hold every input of the table at once. It cannot on every check: one world with every input (one session, sends, forks, sections, runs, the project and its workspaces) and one command held is 2,765,574 states and took 818 s in an optimized build, hours in the build tests run in, and with two commands held it did not finish. Meanwhile (#465) the tests run five exhaustive worlds that together hold every input, each leaving out a kind with the reason no row it keeps reads it, a seeded walk over the one world with every input, and that world exhaustively as an ignored test. To decide: whether R18 is met by worlds that together hold every input, or must name one world; if one, the table's tests need an optimized build of their own and minutes on every check.

## Resolved recently (optional, trimmed in planning passes)
