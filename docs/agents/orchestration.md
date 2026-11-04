# Orchestration

How the orchestration session runs the engineering loop: cut tickets, start implementing sessions, review, integrate. Read this when you are that session; implementing sessions follow `.claude/skills/implement-ticket/SKILL.md` instead.

## Cut tickets

1. Work from a `ready` brief. Use `to-tickets`: slice into issues sized to one fresh context, each with Brief, Requirements, Area, Test seams, Done when, and blocking links. Two tickets that run in parallel touch different areas; shared contracts go first as their own ticket. Before showing the breakdown, check that every field, id, code and default a ticket's tests will assert is named in a brief; one that is not gets its planning pass first, since an unnamed shape becomes an inbox entry, a planning commit on the branch and a CI rerun. A ticket that would change more than about 500 lines is two tickets: the tickets over 1,000 lines (#55, #56, #57) each took three merge rounds, the small ones one. A manual check that needs the running app or the owner's own configuration is marked "owner check" and is the owner's at merge.
2. Show the owner the breakdown (titles, areas, blockers) for approval, then publish every issue with the `ready-for-agent` label. The owner does not approve individual issue bodies.

## Start implementing sessions

- One session per unblocked ticket, each in its own git worktree, never in the integration checkout.
- The prompt is short: "Implement issue #n with the `implement-ticket` skill", plus anything the issue cannot carry (a merged PR to build on, a decision from the owner).

## Review a pull request

1. Read the description, the review section, and the inbox entries.
2. Run the `review-pr` skill on `git diff origin/main...origin/<branch>`: it names the reviewer agents in `.claude/agents/`, the worktree rule (reviewers never check out a branch in the integration checkout), the mutation step and the dismissal rule. Give reviewers what the skill's step 2 names, not the implementer's conclusions.
3. Send every confirmed finding of a round to the implementing session in one message: a second message queues behind the session's turn and may never be read (#48's medium waited a round that way). Before merging, check the reply's fix list against the findings sent. Re-review the fix commits with the skill's step 6.
4. For each finding, name the smallest change that removes the defect; the skill's step 5 says when a fix is a new mechanism and becomes its own ticket. A mechanism kept because the pull request is unsafe without it is re-reviewed whole (the skill's step 6): #71's round-one stage reducer drew a high and then a medium in the next two rounds.
5. Stop after three fix rounds. If the third still finds a defect of medium severity or worse, take the choice to the owner (merge and follow up, or another round) rather than sending a fourth.

## Resolve inbox entries

Planning commits for a pull request go onto its branch, which is checked out in the implementer's worktree. Use a detached worktree:

1. `git worktree add --detach ../ersa-plan origin/<branch>`, then `node scripts/role.mjs planning` inside it.
2. `git merge origin/main` if the branch is behind. Briefs, `docs/MANIFEST.md` and the ADR index merge with the union driver (`.gitattributes`), so two planning commits adding a Changes line or a requirement keep both; read the merged section once, then regenerate the manifest.
3. Update the briefs, delete the resolved inbox lines, run `node scripts/manifest.mjs --check`, commit, `git push origin HEAD:<branch>`.
4. Tell the implementing session to `git pull` before it pushes again. Remove the worktree when done.

## Merge

- Merge only when CI is green on a head that contains main's current tip (`git merge-base --is-ancestor origin/main origin/<branch>`) and the review is clean: `gh pr merge <n> --merge` (merge commits keep `Implements:` trailers). GitHub deletes the branch. A branch behind main gets main merged in first and its CI run again: CI never tests the merge commit itself, and #61 landed red on main because its branch predated #56's test literal.
- After the merge, `git pull` in the integration checkout and report to the owner in a few lines.

## Train the harness

After each merge, compare what the merge-time reviewers found with what the implementing session's own review caught. For each defect that got through, ask which part of the harness should have stopped it (a brief requirement, a reviewer agent's prompt, a test convention, a check in `scripts/`, the hook) and change that part in a planning commit, so the lesson outlives the session. Prune at the same time: a rule nobody needs, a duplicated instruction, a stale note.
