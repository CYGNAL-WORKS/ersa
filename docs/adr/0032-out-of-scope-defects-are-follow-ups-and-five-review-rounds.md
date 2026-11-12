# 0032: A defect outside a change's scope is a follow-up, and the merge review allows five rounds

- Status: accepted
- Date: 2026-11-12
- Related briefs: none

## Context

The merge review blocked on every confirmed high or medium defect, including ones the change neither caused nor was meant to fix: #275 sat past its round limit on a late session name that predated its ticket. ADR 0021's rule that "pre-existing" is no reason to dismiss a finding stands, since the dismissed case can be the defect a ticket exists to fix, but a defect outside the ticket held a merge that could not fix it. Separately, Docket sent a pull request to the owner after three rounds with blocking findings (ADR 0030), and the owner was asked "another round or not" again and again. Sessions now keep fixing past the limit while each round finds new defects, and decide product details themselves (`CLAUDE.md`, Decide the details).

## Options considered

1. **Keep blocking on every defect the review confirms.** Nothing escapes review. A pull request waits on a defect it was never meant to fix.
2. **A defect outside the change's scope becomes a follow-up.** The pull request merges with what it was meant to do, and the defect gets its own ticket. The defect stays on `main` until then.
3. **Raise the round limit only.** Fewer questions for the owner. Rounds still go to defects the pull request cannot fix.

## Decision

Option 2, and the round limit goes from three to five. A defect the change did not introduce, make reachable or make worse, and that lies outside the requirements its ticket builds, is reported as low with the severity it would have in scope, and listed as a follow-up. Docket's `review.maxRounds` is five.

## Consequences

Follow-ups carry real defects, so none may be lost: the harness pass files each as an issue. A pull request reaches the owner after five rounds instead of three, and the rounds it spends go to defects it is answerable for. Revisit if a follow-up defect reaches users before its ticket does, or if pull requests often pass five rounds.
