# 0030: Docket runs the merge review, and authors check their plan's assumptions instead of reviewing their diff

- Status: accepted
- Date: 2026-11-07
- Supersedes: 0023, 0029
- Related briefs: none

## Context

ADR 0023 made the repository public under an organization, put CI on hosted runners and merges through the merge queue, and ran the merge review on the owner's machine through three scripts: a poller, the review, and the gate. ADR 0029 let the poller run three reviews at once. The scripts kept growing (retries, superseded rounds, auto-merge, rate limits), every session polled GitHub for its own state, and the review protocol lived in a skill that a headless Claude Code run followed as prose.

Each change was also reviewed twice by the same protocol: by its author before the pull request opened, and again at merge. The merge review re-found the same classes of defect the author's review looked for, while the most expensive defects, a wrong reading of a brief or a wrong belief about how a peer behaves, surfaced only after the code was built. Six reviewer agents ran on most changes.

## Options considered

1. **Keep the scripts and both review layers.** Nothing to migrate. Keeps the double review, the scripts' upkeep, and sessions polling GitHub.
2. **Docket runs the merge review; authors still review their diff.** Moves the mechanism into typed, tested code. The diff is still reviewed twice.
3. **Docket runs the merge review as the only review; authors check their plan's assumptions before coding.** One review per head, recorded on GitHub. The author's check moves to where a wrong assumption is cheapest to fix and to what the diff review cannot see. More defects show up as merge review rounds.

## Decision

Option 3.

- **Docket**, a local review service on the owner's machine, owns the merge review: syncing pull requests, running reviews in a fixed number of slots, verifying findings, posting, the commit status, the merge gate and the enqueue. The slot count and time limits are its machine configuration, not a record here. The repository owns what to look for, read from `main`: `.claude/review.json` (reviewers, when each runs, gate rules), `REVIEW.md` (the rubric, carrying ADR 0021's severities) and the reviewer agents it names. Both are owner-only paths.
- **Four reviewer agents**: Spec (the issue and briefs, and whether each citing test fails without the change), Code (logic, failure behavior, trust boundaries), Surface, and Design on interface changes. Each review records which reviewers found each finding; the harness pass counts them, and a reviewer that changes no merge decision is narrowed, merged or dropped.
- **No author review.** An implementing session writes its plan with an explicit list of assumptions, and a fresh read-only agent tries to disprove each before code is written. The session then runs the machine checks, opens the pull request, and asks Docket's own endpoint for its state, never GitHub; Docket holds the request until there is something to act on, so a session installs nothing and runs no polling loop of its own.
- The merge review scripts and the `review-pr` skill retire. ADR 0021's severities and narrow re-review stand, with one layer instead of two. From ADR 0023, the public repository, hosted CI, the merge queue, the commit status in place of a check run, reviews under the owner's account, and reviewing only this repository's pull requests all stand.

## Consequences

The first merge review round of a pull request now finds what the author's review used to, so more pull requests take a findings round; the three-round limit stays until the harness pass shows it binds. A reviewer of a fix can no longer be told that a mechanism was kept because the branch was unsafe without it; it reads the fix commits like any other. Sessions depend on Docket running: when it is down they stop and report instead of guessing. Revisit if escaped defects rise, if rounds per pull request grow past the limit often, or when Ersa can run the review as one of its own sessions.
