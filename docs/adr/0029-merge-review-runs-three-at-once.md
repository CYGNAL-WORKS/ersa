# 0029: The merge review runs three reviews at once

- Status: superseded by 0030
- Date: 2026-11-07
- Supersedes: 0027
- Related briefs: none

## Context

ADR 0027 set the merge review poller to two reviews at once and said to revisit the number if the queue still waited on a busy day. Over the rail and composer batch of 2026-11-05 to 11-07, waiting for a verdict was 37 percent of the time pull requests spent open, the largest share; the time from a head's push to its verdict was 37 minutes at the ninetieth percentile with nine pull requests open at once. A review takes from 5 minutes to about an hour (ADR 0027), so the tail is heads waiting for a slot. Each review builds and tests a pull request on the owner's machine, which also runs the implementing sessions.

## Options considered

1. **Keep two slots.** No more load on the owner's machine. A busy afternoon keeps queuing.
2. **Three slots.** Half again the throughput on a busy day; a third review checkout and its build output on the owner's disk; three builds and test runs at once beside the sessions.
3. **Four or more.** More throughput, at a load on the machine nothing has measured.

## Decision

Option 2. The poller runs up to three reviews at once, each in its own checkout. Heads are due in the order their CI passed; a pull request under review is not reviewed again until that review ends, whatever its head now; the scheduled task's run starts no review after its first 45 minutes, so it ends within the task's two-hour limit. ADR 0023 stands.

## Consequences

A third review checkout sits beside the first two. Revisit the number if three reviews slow each other down, measured as a review's own run time growing, or if the push-to-verdict tail stays above half an hour with every slot full.
