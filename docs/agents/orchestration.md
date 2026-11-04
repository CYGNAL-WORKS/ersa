# Orchestration

How the orchestration session runs the engineering loop: cut tickets, start implementing sessions, review, integrate. Read this when you are that session; implementing sessions follow `.claude/skills/implement-ticket/SKILL.md` instead.

## Cut tickets

1. Work from a `ready` brief. Use `to-tickets`: slice into issues sized to one fresh context, each with Brief, Requirements, Area, Test seams, Done when, and blocking links. Two tickets that run in parallel touch different areas; shared contracts go first as their own ticket. Before showing the breakdown, check that every field, id, code and default a ticket's tests will assert is named in a brief; one that is not gets its planning pass first, since an unnamed shape becomes an inbox entry, a planning commit on the branch and a CI rerun. A ticket that would change more than about 500 lines is two tickets: the tickets over 1,000 lines (#55, #56, #57) each took three merge rounds, the small ones one. A manual check that needs the running app or the owner's own configuration is marked "owner check" and is the owner's at merge.
2. Show the owner the breakdown (titles, areas, blockers) for approval, then publish every issue with the `ready-for-agent` label. The owner does not approve individual issue bodies.

## Start implementing sessions

- One session per unblocked ticket, each in its own git worktree, never in the integration checkout.
- The prompt is short: "Implement issue #n with the `implement-ticket` skill", plus anything the issue cannot carry (a merged PR to build on, a decision from the owner).

## Review a pull request

The merge review workflow (`.github/workflows/review.yml`, ADR 0022) runs the `review-pr` skill on every head whose CI passed and posts its findings as a review on the pull request; a high or medium finding makes the Merge review check red. Each round:

1. Read the review and the inbox entries. When the implementing session is not already watching the pull request (`implement-ticket` step 4 has it wait for the check), send it every finding of the round in one message: a second message queues behind the session's turn and may never be read (#48's medium waited a round that way). Check the reply's fix list against the findings sent.
2. Run `review-pr` by hand only when asked, on `git diff origin/main...origin/<branch>`, in a detached worktree under `.scratch/`; reviewers never check out a branch in the integration checkout, and get what the skill's step 2 names, not the implementer's conclusions.
3. The implementing session fixes each finding with the smallest change; the skill's step 5 says when a fix is a new mechanism and becomes its own ticket. A mechanism kept because the pull request is unsafe without it is re-reviewed whole (the skill's step 6): #71's round-one stage reducer drew a high and then a medium in the next two rounds.
4. Stop after three fix rounds. If the third still finds a defect of medium severity or worse, take the choice to the owner (merge and follow up, or another round) rather than sending a fourth.

## Resolve inbox entries

Planning commits for a pull request go onto its branch, which is checked out in the implementer's worktree. Use a detached worktree:

1. `git worktree add --detach ../ersa-plan origin/<branch>`, then `node scripts/role.mjs planning` inside it.
2. `git merge origin/main` if the branch is behind. Briefs, `docs/MANIFEST.md` and the ADR index merge with the union driver (`.gitattributes`), so two planning commits adding a Changes line or a requirement keep both; read the merged section once, then regenerate the manifest.
3. Update the briefs, delete the resolved inbox lines, run `node scripts/manifest.mjs --check`, commit, `git push origin HEAD:<branch>`.
4. Tell the implementing session to `git pull` before it pushes again. Remove the worktree when done.

## Merge

- The workflow merges through the gate (`scripts/merge-pr.mjs`, merge commits, so `Implements:` trailers stay) when the review is clean, CI is green and the head contains main's tip. A pull request that changes an owner-only path (workflows, hooks, instruction files, the review or the gate), is labeled `needs-owner`, and the owner merges it; a bot's pull request (a dependency bump) gets the label without a review, since taking a new version is the owner's call (ADR 0015); one labeled `hold` is reviewed and never merged by the workflow. A branch behind main gets main merged in and pushed by whoever is driving it, and CI and the review run again: CI never tests the merge commit itself, and #61 landed red on main because its branch predated #56's test literal.
- After a merge, `git pull` in the integration checkout and report to the owner in a few lines.

## Train the harness

After each merge, compare what the merge review found with what the implementing session's own review caught. For each defect that got through, ask which part of the harness should have stopped it (a brief requirement, a reviewer agent's prompt, a test convention, a check in `scripts/`, the hook) and change that part in a planning commit, so the lesson outlives the session. Prune at the same time: a rule nobody needs, a duplicated instruction, a stale note.
