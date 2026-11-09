# Orchestration

How the orchestration session runs the engineering loop: cut tickets, start implementing sessions, review, integrate. Read this when you are that session; implementing sessions follow `.claude/skills/implement-ticket/SKILL.md` instead.

## Cut tickets

1. Work from a `ready` brief. Use `to-tickets`: slice into issues sized to one fresh context, each with Brief, Requirements, Area, Test seams, Done when, and blocking links. Two tickets that run in parallel touch different areas; shared contracts go first as their own ticket. Before showing the breakdown, check that every field, id, code and default a ticket's tests will assert is named in a brief; one that is not gets its planning pass first, since an unnamed shape becomes an inbox entry, a wait at the merge gate and a planning commit on the branch. The breakdown shows those names per ticket with the brief that names each, and a size estimate; a ticket that would change more than about 500 lines is two tickets, split before the owner approves. A check that needs the running app is a live check: the implementing session drives the real window with Playwright (the live-check harness, `docs/agents/live-checks.md`), answering native dialogs and drops from the test, and puts its screenshots in the pull request. Only a check that needs the owner's own judgment is marked "owner check" and is the owner's at merge.
2. Show the owner the breakdown (titles, areas, blockers) for approval, then publish every issue with the `ready-for-agent` label. The owner does not approve individual issue bodies.

## Start implementing sessions

- One session per unblocked ticket, each in its own git worktree, never in the integration checkout.
- The prompt is short: "Implement issue #n with the `implement-ticket` skill", plus anything the issue cannot carry (a merged PR to build on, a decision from the owner).

## Review a pull request

Docket runs the merge review on the owner's machine (ADR 0030): it reviews each head whose CI passed with the reviewers `.claude/review.json` names, against the `REVIEW.md` rubric, verifies their findings, posts them as a review on the pull request, and runs the merge gate. A pull request gets no other review: the implementing session checks its plan's assumptions before coding (`implement-ticket` step 2) and leaves the diff to Docket. Docket's page (`http://127.0.0.1:4477`) shows its queue and running reviews; its README, in the Docket repository (`KPandaK/docket`), says how it decides what is due, retries and cancels. Each round:

1. The implementing session polls Docket for its own pull request and fixes what the review posts (`implement-ticket` step 5); forward nothing to it while it does. Read its end-of-turn report and relay to the owner only what it escalates: three rounds spent, a pull request waiting on the owner, Docket or CI not running, or a pull request no session owns. A session whose pull request waits on a planning pass messages you when it stops: run the pass at once (Resolve inbox entries), and take to the owner only an entry the briefs cannot settle without their call. Only when a session is not polling its pull request (its turn ended before the review posted), send it every finding of the round in one message: a second message queues behind the session's turn and may never be read.
2. To see where any pull request stands, ask Docket (`curl -sS http://127.0.0.1:4477/pulls/<n>`); never poll GitHub for it.
3. Stop after three fix rounds. If the third still finds a defect of medium severity or worse, take the choice to the owner (merge and follow up, or another round) rather than sending a fourth.

## Resolve inbox entries

The merge gate holds a pull request while its inbox has an open entry (ADR 0025): its review is clean but its status stays pending, naming the entries. Planning commits for a pull request go onto its branch, which is checked out in the implementer's worktree; a separate planning pull request costs a review, a queue merge and a merge of main on the ticket's branch, and is made only for a shape two in-flight tickets share, which must land on `main` once. Whoever opens one drives it to merge as `implement-ticket` steps 4 to 6 drive a ticket's pull request. A brief's "Built so far" follows `docs/briefs/README.md`: it rides the ticket's branch, never a pull request that merges ahead of the code. Use a detached worktree:

1. `git worktree add --detach ../ersa-plan origin/<branch>`, then `node scripts/role.mjs planning` inside it.
2. `git merge origin/main` if the branch is behind. Briefs, `docs/MANIFEST.md` and the ADR index merge with the union driver (`.gitattributes`), so two planning commits adding a Changes line or a requirement keep both; read the merged section once, then regenerate the manifest.
3. Update the briefs as `docs/briefs/README.md` (Changing a brief) says, delete the resolved inbox lines, run `node scripts/manifest.mjs --check`, commit, `git push origin HEAD:<branch>`.
4. Tell the implementing session the pass is pushed: it pulls and waits on Docket for the new head (`implement-ticket` step 6). Remove the worktree when done.

## Merge

- Docket adds a clean head to the merge queue when the gate passes: CI green, no open inbox entry, nothing pushed after the review but clean merges of `main`. The queue builds and tests the merge before `main` moves and merges with a merge commit, so `Implements:` trailers stay (ADR 0030); a branch behind main needs no merge of main first. A head the queue removes (CI failed on the merge, or a conflict) needs a push, and CI and the review run again.
- A clean pull request that changes an owner-only path (`gate.ownerPaths` in `.claude/review.json`: workflows, hooks, instruction files, the review config and rubric, the manifest) is labeled for the owner, who merges it. So is a bot's pull request (a dependency bump), without a review, since taking a new version is the owner's call (ADR 0015). One labeled `hold` is reviewed and never enqueued, and Docket turns off auto-merge armed on a held or unreviewed head.
- After a merge, `git pull` in the integration checkout and report to the owner in a few lines.
- When a batch's last ticket merges, cut one more: a retirement ticket for the surface the batch replaced (a control moved elsewhere, a list a tree superseded), found by reading the batch's briefs' "Built so far" against the interface. Surface no ticket retires is what the owner finds by hand.

## Train the harness

Merges are automatic, so the harness is trained in batches. Run a pass in a planning session when the owner asks. When you report a merge, offer one if 10 or more pull requests have merged since the last pass and no pass's pull request is open (`gh pr list --author @me --search "Harness: lessons from in:title"`). The last pass is the newest commit on `main` whose subject starts `Harness: lessons from`; `git log --first-parent --merges --oneline <that commit>..origin/main` lists the pull requests merged since; leave out the pass's own.

1. Run `node scripts/harness-lessons.mjs`: for each of those pull requests it lists the merge review's blocking findings with the reviewers that found each, and counts them per reviewer.
2. For each blocking finding, and each defect found on `main` since the last pass, which the review missed, change the cheapest part of the harness that would have stopped it before the pull request opened, in this order: a check in `scripts/` or the hook, a test convention, the assumption checker, a brief requirement. A missed defect also changes the reviewer agent whose concern it was, or `REVIEW.md`.
3. Take up the follow-ups each pull request lists: make each now, or file it as a `needs-triage` issue, so none is lost.
4. Prune at the same time: a rule nobody needs, a duplicated instruction, a stale note, and a reviewer-prompt clause whose findings changed no merge decision (ADR 0021). A reviewer agent the count shows at zero across two passes is narrowed, merged into another, or dropped from `.claude/review.json`.
5. Commit with a subject that starts `Harness: lessons from`, one body line per change naming the pull requests behind it, and title the pass's pull request with that subject. That commit is the record of the pass.
