---
name: shepherd-prs
description: Move a set of open pull requests toward merge from a dedicated, low-effort session that sleeps until the queue changes. Use when asked to shepherd, babysit or watch the pull request queue.
---

# Shepherd pull requests

You run in a session of your own, at low effort. `scripts/pr-queue.mjs` does the watching and the triage, with no model; you wake only when it reports a change, act on the rows that changed, and sleep again. Keep every turn short: you hand work to whoever has the context, and you never read a full CI log yourself. Message a session only when it must act now, with everything for it in one message: every message wakes it for a full turn, and one idle for over an hour rereads its whole context. A fact it needs only later goes on the pull request or in your report.

## Start

1. Run `node scripts/pr-queue.mjs --changed`. It prints every open pull request with its state and, for a red one, the failure's signature; this run sets the baseline.
2. Name the run's **set**: the pull requests it printed, minus any the owner left out, and minus any another running session says it is driving (an orchestration session, or one that said it will rerun or merge them); search the app's session transcripts for their numbers. Name those to the owner and leave them unless the owner gives them to you; two sessions moving one pull request push over each other. Write the set, the done condition ("every pull request in the set is merged, closed, or on the owner's list") and the budget to `.scratch/pr-queue/run.md`. A pull request opened later is not in the set; mention it once in your report and leave it.
3. Act on every row of the set (below), then sleep.

## Act on a row

- **ci-running**, **draft**: nothing.
- **ci-red, shared** (two or more pull requests fail the same way, usually a runner or a flaky test): one cause, so one action.
  - If main has a fix for it since the pull requests' runs, merge main into each branch that is behind (see behind-main) or, where the branch already contains main, rerun its failed jobs once (`gh run rerun <run id> --failed`). Never rerun the same head commit twice.
  - Otherwise put the cause on the owner's list once, naming the pull requests it blocks.
- **ci-red** with failed tests or a build error of its own: send the signature and the run's link to the session that owns the branch, in one message, and leave the fix to it. Find that session with the app's session list; if none is running, put it on the owner's list. A policy failure (a dependency too new, a license) goes on the owner's list.
- **behind-main**, **conflicting**: if the owning session is running, ask it to merge main. Otherwise merge `origin/main` into the branch in a detached worktree under the scratchpad, as `docs/agents/orchestration.md` does for planning commits, and push. A conflict outside the union-merged files goes on the owner's list.
- **green**: run the merge review in `docs/agents/orchestration.md` (the `review-pr` skill) in a subagent at the stronger model, and send confirmed findings to the owning session. When the review is clean, merge only if the owner said this run may merge; otherwise put it on the owner's list as ready.

## Sleep

Start, in the background, `until node scripts/pr-queue.mjs --changed; do sleep 600; done`, and end your turn. It prints nothing until the queue changes, and its exit wakes you. Never poll it, never schedule a wakeup to check on it, and never run it in the foreground.

On waking, read only the "Changed since the last run" rows for pull requests in the set, act on those, and sleep again.

## Stop

Stop, and report to the owner with one line per pull request in the set, when:

- the done condition holds;
- three wakes in a row moved no pull request in the set forward (a state change, a merge, a hand-off); say what blocks them;
- the run has woken twelve times.

The owner's list is part of every report: what needs their decision, once each, with the pull requests it blocks.
