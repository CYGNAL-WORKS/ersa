# 0022: The merge review is a GitHub Actions workflow, and nothing fixes automatically

- Status: accepted
- Date: 2026-11-04
- Related briefs: none

## Context

The review and merge pipeline had three generations layered on each other: a session-driven shepherd skill with a queue script, and an open pull request with a 1,600-line headless job runner (a loop, a job script and a merge gate) that reviewed, fixed, verified and merged with no session waiting. Its 37 job records cost about $45 and reported 56 medium or high findings over eight pull requests. Small pull requests (a hook, a 200-line script) reached a clean review in one or two rounds. The runner's own pull requests, 1,000 to 1,600 lines of process supervision, worktree lifecycle, retry accounting, GitHub comment and label state and a decision table, never did: every automated fix round added code the next review found real defects in (hooks disabled so the role guard enforced nothing, nested instruction files escaping the owner-only path check, process trees leaking on Windows, a restarted loop deleting a live job's worktree, comments ignored). The review loop was working; its subject was the problem, and it broke the project's own rules: tickets stay under 500 lines, and a new mechanism becomes its own ticket. The runner rebuilt what GitHub Actions and GitHub already provide: a job per event, a timeout, cancellation of a superseded run, a record on the pull request. The repository is private on GitHub Free, so there is no branch protection, no ruleset and no merge queue; the merge rule has to be a script. ADR 0021 defines severity and says each layer starts with one full review and re-reviews only the commits since.

## Options considered

1. **Fix the shepherd.** Keep the loop and close the findings. Keeps automatic fix rounds. Three rounds of fixes to its own supervision code each drew new mediums; the next would too, and every line of it is ours to keep.
2. **Drop the merge-time review to highs only.** Converges by lowering the bar. Merges the mediums the author's review missed, against ADR 0021.
3. **Move the review and the merge into a workflow; no automated fixes.** A `workflow_run` job on the owner's Windows runner, where Claude Code is installed and logged in, runs the `review-pr` skill on each head whose CI passed, posts the findings as a review, and merges through the gate when the review is clean. Findings go back to the implementing session, which has the ticket's context and fixes them, up to three rounds; then the owner decides. Actions supplies the queue, the timeout, the cancellation and the token; GitHub supplies the record. About 200 lines of script remain, plus the gate.

## Decision

Option 3. The shepherd skill, the queue script and the job runner are retired; the merge gate stays. The workflow file, the review script, the gate, the hook scripts and the instruction files all come from main, so a pull request cannot change its own reviewer; a pull request that changes a workflow, an instruction file, a hook, the review or the gate is labeled `needs-owner` and the owner merges it. Reviews are posted as comments, never approvals, since Actions may not approve pull requests here.

## Consequences

No automated fix rounds, so an implementing session stays until its pull request merged, waits on the owner or spent its three rounds; a finding on a session that has gone is the orchestration session's to relay. The review runs on the owner's machine with the owner's Claude login, and the pull request's own tests run there too, with the workflow's write token in the environment and the owner's stored credentials within reach: a hostile pull request from a collaborator could push past the review, which the shepherd, running with the owner's full login, allowed just the same. Only pull requests from this repository are reviewed, never a fork. GitHub reviews are the only record: there is no state file to lose or to desynchronize, and nothing to read when the pull request is gone. Each pull request's review costs what one `review-pr` run costs, with no fix jobs after it. A merge made with the workflow's token starts no CI run on main; the head it merged contained main's tip and passed CI, so main holds what CI tested. Branch protection would replace the gate if the repository ever leaves GitHub Free. Revisit if reviews stop converging within three rounds, or if the runner's availability holds pull requests up.
