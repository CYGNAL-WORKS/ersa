# 0027: The merge review runs two reviews at once, the longest-waiting head first

- Status: accepted
- Date: 2026-11-06
- Related briefs: none

## Context

ADR 0023 has the merge review poller review each pull request whose CI passed "one at a time", and the poller took them in the order `gh pr list` returns, newest first. A review takes 5 to 40 minutes. On 2026-11-06, #175's head passed CI at 15:23 and was still waiting at 17:33: four newer pull requests drew blocking findings, were pushed again, and went back to the front each time. The head that waited longest was reviewed last.

## Options considered

1. **Longest-waiting head first, one at a time.** Ends the starvation. Throughput stays at one review at a time, so a busy afternoon still queues for hours.
2. **Two at once, list order.** Doubles throughput. Pull requests pushed again and again can still fill both slots and keep an older one waiting.
3. **Two at once, longest-waiting head first.** Each slot has its own checkout. Both reviews build and test on the owner's machine, so they compete for CPU and disk.
4. **More than two at once.** More throughput, at the cost of the owner's machine, which also runs the implementing sessions.

## Decision

Option 3. Heads are due in the order their CI passed. The poller runs up to two reviews at once, each in its own checkout, and a pull request under review is not reviewed again until that review ends, whatever its head now. The rest of ADR 0023 stands.

## Consequences

A second review checkout sits beside the first on the owner's disk, with its own build output. The scheduled task's run stays alive while its reviews run, filling a slot that frees up, and starts none after its first hour, so it ends within the task's two-hour limit. Revisit the number of slots if reviews slow each other down, or if the queue still waits on a busy day.
