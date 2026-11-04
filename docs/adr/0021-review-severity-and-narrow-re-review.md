# 0021: Review severity is defined by who gets hurt, and a re-review checks only what changed

- Status: accepted
- Date: 2026-11-03
- Related briefs: none

## Context

The script-driven shepherd ran 33 review and fix jobs over eight pull requests and reported 56 medium or high findings. Few pull requests reached a clean review: each round found new mediums, and each fix added code and tests the next round picked apart. About a third of the mediums were tests that would pass without the change, and many more were robustness gaps in scripts only the owner runs, rated the same as a defect that merges unreviewed code. Full re-reviews after every new head, including merges of main with a hand-resolved conflict, and stacked pull requests reviewed once per layer, kept the search open. Findings on paths a headless job cannot edit, or on contradictions between harness rules, spent fix rounds before they reached the owner.

## Options considered

1. **Keep the rules and raise the round limit.** Nothing converges sooner; usage grows with every round.
2. **Let only high findings block a re-review.** Converges, but merges real defects the fix itself introduced, against `review-pr`'s rule that mediums are fixed.
3. **Define severity by consequence and narrow every re-review; mediums keep blocking.** The search ends because each round reads less, not because the bar drops.

## Decision

Option 3.

- **High:** a wrong result, data loss or a security hole for the user, or unreviewed code reaching main.
- **Medium:** a defect the user, or the loop itself, meets in normal use. A test that passes without the change is medium only when it cites a requirement or covers the fix of a high or medium finding; otherwise low. Mutation targets requirement values and fix hunks, not arbitrary lines.
- **Low:** everything else, including a gap in owner-run tooling that fails loudly.

High and medium findings block, whichever round found them. After the first full review, a re-review reads only the commits since it, and of a merge of main only its conflict resolution; a stacked pull request is reviewed on its own commits. A finding the loop cannot act on (a planning-only path, a contradiction between harness rules) goes to the owner at once and spends no fix round.

## Consequences

Reviews converge in a bounded number of rounds without lowering the bar for what they do read. A defect outside the fix commits that a first review missed is found later or not at all; when one gets through, "Train the harness" turns it into a check or a reviewer-brief clause. The same step now also prunes: a brief clause whose findings changed no merge decision is narrowed or removed. The `review-pr` skill, `docs/agents/orchestration.md` and the shepherd carry these rules once the reviewer-agent and shepherd pull requests merge. Revisit if escaped defects rise or rounds still fail to converge.
