# Orchestration

How the orchestration session runs the engineering loop: cut tickets, start implementing sessions, review, integrate. Read this when you are that session; implementing sessions follow `.claude/skills/implement-ticket/SKILL.md` instead.

## Cut tickets

1. Work from a `ready` brief. Use `to-tickets`: slice into issues sized to one fresh context, each with Brief, Requirements, Area, Test seams, Done when, and blocking links. Two tickets that run in parallel touch different areas; shared contracts go first as their own ticket.
2. Show the owner the breakdown (titles, areas, blockers) for approval, then publish every issue with the `ready-for-agent` label. The owner does not approve individual issue bodies.

## Start implementing sessions

- One session per unblocked ticket, each in its own git worktree, never in the integration checkout.
- The prompt is short: "Implement issue #n with the `implement-ticket` skill", plus anything the issue cannot carry (a merged PR to build on, a decision from the owner).

## Review a pull request

1. Read the description, the review section, and the inbox entries.
2. Run independent reviewers on the diff (`git diff main...origin/<branch>`): a correctness reviewer always, plus failure-mode or security reviewers where the change touches processes, pipes, files or untrusted input. Give them the diff and the briefs, not the implementer's conclusions.
3. Send confirmed findings back to the implementing session as one message, and re-review the fix commit.
4. Ask each fix round for the smallest change that removes the defect. A finding whose fix needs a new mechanism (a cache, a background task, a protocol field) becomes its own ticket unless the pull request is unsafe to merge without it: new mechanisms bring their own defects, and the next round reviews those instead of converging. (PR #42: a cache asked for in round 1 drew a new finding in each of the next two rounds.)
5. Stop after three fix rounds. If the third still finds a defect of medium severity or worse, take the choice to the owner (merge and follow up, or another round) rather than sending a fourth.

## Resolve inbox entries

Planning commits for a pull request go onto its branch, which is checked out in the implementer's worktree. Use a detached worktree:

1. `git worktree add --detach ../ersa-plan origin/<branch>`, then `node scripts/role.mjs planning` inside it.
2. `git merge origin/main` if the branch is behind; resolve `docs/MANIFEST.md` by regenerating it.
3. Update the briefs, delete the resolved inbox lines, run `node scripts/manifest.mjs --check`, commit, `git push origin HEAD:<branch>`.
4. Tell the implementing session to `git pull` before it pushes again. Remove the worktree when done.

## Merge

- Merge only when CI is green and the review is clean: `gh pr merge <n> --merge` (merge commits keep `Implements:` trailers). GitHub deletes the branch.
- After the merge, `git pull` in the integration checkout and report to the owner in a few lines.

## Train the harness

After each merge, compare what the merge-time reviewers found with what the implementing session's own review caught. For each defect that got through, ask which part of the harness should have stopped it (a brief requirement, a skill's reviewer brief, a test convention, a check in `scripts/`, the hook) and change that part in a planning commit, so the lesson outlives the session. Prune at the same time: a rule nobody needs, a duplicated instruction, a stale note.
