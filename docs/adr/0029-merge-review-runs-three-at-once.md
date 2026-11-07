# 0029: The merge review runs three reviews at once

- Status: accepted
- Date: 2026-11-07
- Supersedes: 0027
- Related briefs: none

## Context

ADR 0027 set the merge review poller to two reviews at once, the longest-waiting head first, and said to revisit the number if the queue still waited on a busy day. Over the rail and composer batch of 2026-11-05 to 11-07 (48 pull requests), the time from a head's push to its verdict was 13 minutes at the median, 37 at the ninetieth percentile and 162 at most, with nine pull requests open at once; waiting for a verdict was 37 percent of the time pull requests spent open, the largest share. A review takes 5 to 40 minutes, so the tail is heads waiting for a slot, not reviews running long. The owner's machine ran the two reviews beside the implementing sessions without the reviews slowing each other noticeably.

## Options considered

1. **Keep two slots.** No more load on the owner's machine. A busy afternoon keeps queuing for an hour or more.
2. **Three slots.** Half again the throughput on a busy day; a third review checkout and its build output on the owner's disk; three builds and test runs at once beside the implementing sessions.
3. **Four or more.** More throughput, but each review builds and tests the pull request, and the machine also runs the sessions; the reviews would slow each other down and the gain is unmeasured.
4. **Review on the hosted runners instead.** Removes the limit, but the review needs the owner's Claude Code login (ADR 0023).

## Decision

Option 2. The poller runs up to three reviews at once, each in its own checkout, heads still due in the order their CI passed. The rest of ADR 0027 and 0023 stands.

## Consequences

A third review checkout sits beside the first two. Revisit the number again if three reviews slow each other down, measured as the review's own run time growing, or if the push-to-verdict tail stays above half an hour with the slots full.
