# 0037: Core decides every lifecycle change in one place, from one table

- Status: accepted
- Date: 2026-11-15
- Related briefs: ARCH-CORE, PROTO-CORE-API

## Context

Core runs every request as its own task and had no stated concurrency model. Order was kept by about twenty separate devices (locks, flags, counters, drop guards, in-memory claims, a compare-and-swap in the database), each added for one pair of operations that could overlap. About a sixth of the month's Core commits fixed such a pair, and the two latest pull requests to touch sessions each needed fixes for new ones. The cause is structural: an operation in progress (a start, an archive, a delete, a removal, a send, a fork) existed only as a held lock, a raised flag or a guard on a request's stack, so each pair of operations needed its own proof, written at the call sites, that each sees the other. The owner asked on 2026-11-15 whether a systematic model should replace this, at whatever cost.

## Options considered

1. **Keep the guards, state one lock order, and test interleavings by machine**: least work, and the tests are worth having. A lock cannot be held across the slow part of an operation without stalling Stop, so every operation still needs its own in-progress flag and every pair its own proof. The pairs get cheaper to check, not fewer.
2. **One owner task per session, and one for what exists**: each session's pairs become rows of a table. Fork, a delete that takes a workspace, removing a workspace or project, and shutdown then span owners, and an owner that asks another a question may act on a stale answer unless the answer is a lease it must return on every way out. Those leases are the claims and holds again, as messages, for exactly the pairs that produce the most findings.
3. **One owner task for all of Core, with a mailbox**: nothing spans owners. To keep "stored first, then kept in memory" the owner must wait for the database inside its step, which makes it a single point every session's Stop and shutdown queue behind. Take the database out of the step and the step never waits, at which point the task and mailbox exclude nothing a lock does not, and cost a reply channel per command, copies for readers, and a migration that cannot be done in slices.
4. **One state, one synchronous transition function, one lock**: the same table and the same single order as option 3, without the task. Every call site can ask in place, so the build can move one group of operations at a time.

## Decision

Option 4. Core keeps one admission state: which projects, workspaces, sessions and sections exist, each workspace's state and holder, each session's stored row, run phase, exclusive operation, send slot and fork mark, the commands being held, and whether Core is closing. It changes only through one function that takes one input, decides it from one table, and returns without waiting or doing I/O.

- An input is a command (begin something) or a report (an admitted operation saying what happened: stored, launched, ended, failed, dropped). The table decides every input in every state explicitly: do it, refuse it, or hold it. There is no default case, so an input the table has no answer for does not compile.
- An admitted operation is recorded in the state, in the session's slot, the workspace's, or both in the same step, and gets a ticket. Its slow work runs outside, written straight-line. Each change it makes to the state is a report, and a stored change is stored before it is reported. The ticket reports the operation's end when its last holder lets go, however the task ends.
- A held command is data in the state. The table sees it, decides it again inside whichever later step changes what it waits for, and answers it. Only commands are held, never reports, so the holder of a ticket never waits on the table and no hold waits on another in a cycle.
- The lock is held for one step only, never across a wait and never while taking another lock.

The data path is outside this decision: a backend's lines through the pipeline and view to subscribers, the raw log, tiles, and requests that only talk to a live backend each have one writer already. What the pipeline learns that a decision depends on (the backend's id for a session, a turn running, a run ending) enters the state as reports.

## Consequences

- Two of Core's own operations can no longer interleave on state admission holds. Between an operation's steps, its work in the outside world is protected by its slot and the table's rows for that slot: still a rule to get right, but one row per input in one function, where it was one proof per pair per call site.
- Eleven of the twenty devices retire, all of them exclusion between Core's own operations, and five kinds of drop guard become one ticket. Nine stay, because they bound the outside world or keep a promise to a client: time limits, the call-off of an abandoned deletion, single-flight reads on a disk that hangs, the folder-inspection limit, the scratch folder's lock, the per-connection request cap, the stored `deleting` and workspace states that survive a crash, and each connection's per-session ordering (PROTO-CORE-API-R92).
- Races with the outside world are not addressed: a backend exiting or starting a turn, a disk that hangs, another program with the session open, git failing part way, Core stopping part way.
- The table is tested by applying inputs to it directly: every input in every reachable state of a small world, with invariants checked in each, and seeded random sequences. That tests the table, not what an operation does between its reports, so operations keep their tests through the Core API, and a seeded schedule test orders the completions of the fake adapter's and transport's calls. Test stops inside product code that existed to land a request inside a window retire with the window.
- No Core API requirement changes meaning. The table keeps what clients see today, including where a single slot would be too blunt: two archives of one session at once both succeed, a session can be removed or archived while a fork of it is being read, a session can be removed while a message to it is still copying, and an archive waiting its turn already refuses sends (PROTO-CORE-API-R85). Outcomes of requests racing from different connections follow the order their steps run; each was already possible.
- A test whose interleaving becomes unreachable is replaced, in the change that makes it so, by a test of the table citing the same requirement.
- New code that changes what exists or what is under way adds an input and its rows. A new lock, flag or guard between two of Core's operations is a defect.
- Revisit if a decision ever needs to wait (it would have to become an operation), or if the lock shows up as contention, which at the rate people start, stop and delete sessions it should not.
