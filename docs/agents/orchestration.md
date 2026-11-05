# Orchestration

How the orchestration session runs the engineering loop: cut tickets, start implementing sessions, review, integrate. Read this when you are that session; implementing sessions follow `.claude/skills/implement-ticket/SKILL.md` instead.

## Cut tickets

1. Work from a `ready` brief. Use `to-tickets`: slice into issues sized to one fresh context, each with Brief, Requirements, Area, Test seams, Done when, and blocking links. Two tickets that run in parallel touch different areas; shared contracts go first as their own ticket. Before showing the breakdown, check that every field, id, code and default a ticket's tests will assert is named in a brief; one that is not gets its planning pass first, since an unnamed shape becomes an inbox entry, a wait at the merge gate and a planning commit on the branch. A ticket that would change more than about 500 lines is two tickets: the tickets over 1,000 lines (#55, #56, #57) each took three merge rounds, the small ones one. A manual check that needs the running app or the owner's own configuration is marked "owner check" and is the owner's at merge.
2. Show the owner the breakdown (titles, areas, blockers) for approval, then publish every issue with the `ready-for-agent` label. The owner does not approve individual issue bodies.

## Start implementing sessions

- One session per unblocked ticket, each in its own git worktree, never in the integration checkout.
- The prompt is short: "Implement issue #n with the `implement-ticket` skill", plus anything the issue cannot carry (a merged PR to build on, a decision from the owner).

## Review a pull request

The merge review runs on the owner's machine (ADR 0023). The merge review poller (`scripts/merge-review-watch.mjs`, run every 5 minutes by a scheduled task; its header says how to register the task and where its log is) runs the `review-pr` skill through `scripts/merge-review.mjs` on one head per tick among those whose CI passed, and posts the findings as a review on the pull request. The review runs the pull request's own tests as the owner, with the owner's `gh` login: only pull requests from this repository are reviewed, never a fork's. The head carries a "Merge review" commit status: pending while the review runs or while a clean head waits for the gate (GitHub still working out the merge, a CI rerun), red when a high or medium finding blocks the merge, and `error` when the run itself failed. The poller takes a pending head up again after an hour. An `error` is the owner's to look at: the log says why, and `node scripts/merge-review.mjs <n> --head <sha>` in the review checkout runs the review again. The status is for people and the poller; the `main` ruleset must not require it, since the queue would wait for it on the merge commit, where nothing sets it. Each round:

1. Read the review and the inbox entries. When the implementing session is not already watching the pull request (`implement-ticket` step 4 has it wait for the status), send it every finding of the round in one message: a second message queues behind the session's turn and may never be read (#48's medium waited a round that way). Check the reply's fix list against the findings sent.
2. Run `review-pr` by hand only when asked, on `git diff origin/main...origin/<branch>`, in a detached worktree under `.scratch/`; reviewers never check out a branch in the integration checkout, and get what the skill's step 2 names, not the implementer's conclusions.
3. The implementing session fixes each finding with the smallest change; the skill's step 5 says when a fix is a new mechanism and becomes its own ticket. A mechanism kept because the pull request is unsafe without it is re-reviewed whole (the skill's step 6): #71's round-one stage reducer drew a high and then a medium in the next two rounds.
4. Stop after three fix rounds. If the third still finds a defect of medium severity or worse, take the choice to the owner (merge and follow up, or another round) rather than sending a fourth.

## Resolve inbox entries

The merge gate holds a pull request while its inbox has an open entry (ADR 0025): its review is clean but its status stays pending, naming the entries. Planning commits for a pull request go onto its branch, which is checked out in the implementer's worktree. Use a detached worktree:

1. `git worktree add --detach ../ersa-plan origin/<branch>`, then `node scripts/role.mjs planning` inside it.
2. `git merge origin/main` if the branch is behind. Briefs, `docs/MANIFEST.md` and the ADR index merge with the union driver (`.gitattributes`), so two planning commits adding a Changes line or a requirement keep both; read the merged section once, then regenerate the manifest.
3. Update the briefs, delete the resolved inbox lines, run `node scripts/manifest.mjs --check`, commit, `git push origin HEAD:<branch>`.
4. Tell the implementing session to `git pull` before it pushes again. Remove the worktree when done.

## Merge

- The merge review adds a clean head to the merge queue through the gate (`scripts/merge-pr.mjs`) when CI is green. The queue builds and tests the merge before `main` moves and merges with a merge commit, so `Implements:` trailers stay (ADR 0023); a branch behind main needs no merge of main first. A head the queue removes (CI failed on the merge, or a conflict) needs a push, and CI and the review run again. A pull request that changes an owner-only path (workflows, hooks, instruction files, the review, the gate, or the poller) is labeled `needs-owner`, and the owner merges it. A bot's pull request (a dependency bump) gets the label without a review, since taking a new version is the owner's call (ADR 0015). One labeled `hold` is reviewed and never enqueued; for a `hold` put on after the head was enqueued, the poller turns auto-merge off on its next tick, and its log says if GitHub refused. A push after the head was enqueued leaves auto-merge on, so the poller turns it off on its next tick; the new head is reviewed and enqueued again when clean.
- After a merge, `git pull` in the integration checkout and report to the owner in a few lines.

## Train the harness

After each merge, compare what the merge review found with what the implementing session's own review caught. For each defect that got through, ask which part of the harness should have stopped it (a brief requirement, a reviewer agent's prompt, a test convention, a check in `scripts/`, the hook) and change that part in a planning commit, so the lesson outlives the session. Prune at the same time: a rule nobody needs, a duplicated instruction, a stale note. Take up the pull request's follow-ups the same way: make each now, or file it as a `needs-triage` issue, so none is lost when the pull request closes.
