# Harness lessons pass

How a planning session trains the harness from the pull requests merged since the last pass. `docs/agents/orchestration.md` (Train the harness) says when to run one.

The last pass is the newest commit on `main` whose subject starts `Harness: lessons from`; `git log --first-parent --merges --oneline <that commit>..origin/main` lists the pull requests merged since; leave out the pass's own.

1. Run `node scripts/harness-lessons.mjs`: for each of those pull requests it lists the merge review's blocking findings with the reviewers that found each, and counts them per reviewer, all and alone.
2. For each blocking finding, and each defect found on `main` since the last pass, which the review missed, change the cheapest part of the harness that would have stopped it before the pull request opened, in this order: a check in `scripts/` or the hook, a test convention, the assumption checker, a brief requirement. A missed defect also changes the reviewer agent whose concern it was, or `REVIEW.md`.
3. Take up the follow-ups each pull request lists: make each now, or file it as a `needs-triage` issue, so none is lost.
4. Prune at the same time: a rule nobody needs, a duplicated instruction, a stale note, and a reviewer-prompt clause whose findings changed no merge decision (ADR 0021). A reviewer agent that found nothing alone across two passes is narrowed, merged into another, or dropped from `.claude/review.json`: a finding another reviewer also made changed no merge decision.
5. Commit with a subject that starts `Harness: lessons from`, one body line per change naming the pull requests behind it, and title the pass's pull request with that subject. That commit is the record of the pass.
