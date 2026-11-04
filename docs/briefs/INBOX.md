# Inbox

Findings from implementing sessions that need a planning pass: a brief that contradicts reality, a decision the brief did not cover, a requirement that turned out untestable as written. Append one line per finding. Do not edit briefs from an implementing session.

Format: `- [ ] YYYY-MM-DD BRIEF-ID (or none): what was found and what was done meanwhile.`

A planning pass resolves each entry by updating a brief or ADR and then deletes the line. The manifest check fails while any `- [ ]` line remains.

## Open

- [ ] 2026-11-04 none: `.claude/skills/implement-ticket/SKILL.md` step 4 still says the merge review runs by hand until the poller lands and names a Merge review check; #142 landed the poller and a commit status. New wording for step 4: "Wait for CI and for the Merge review status (`gh pr checks <n> --watch`). CI runs on GitHub-hosted Linux and Windows runners: a test that assumes a platform's path form or locale fails on the other one. The merge review runs on the owner's machine (ADR 0023): a poller picks up every head whose CI passed within a few minutes while that machine is on, and the status is pending while the review runs. When CI is green and the status is not listed after a few minutes, report that the pull request waits for the merge review rather than watch indefinitely. Fix CI failures and push until it is green. The merge review posts its findings as a review on the pull request: fix every high and medium with the smallest change, push, and wait again, up to three rounds. After the third, stop and report to the owner in plain English what remains." Step 5's "when the pull request merged" becomes "when the pull request merged or entered the merge queue". Done meanwhile: `docs/agents/orchestration.md` describes the poller and the status.
- [ ] 2026-11-04 none: `docs/GLOSSARY.md`, **Merge review**, says the merge review workflow posts it and that a pull request merges when it is clean; under ADR 0023 the merge review poller posts it, the head carries a "Merge review" commit status, and a clean pull request enters the merge queue. Proposed: "the review the merge review poller posts on a pull request's head once CI passes, running the same protocol the author ran before opening it, recorded on the head as a commit status. A pull request enters the merge queue when its merge review is clean." Done meanwhile: nothing; the term is used as is.

## Resolved recently (optional, trimmed in planning passes)
