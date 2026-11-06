---
name: review-pr
description: Review a diff adversarially with parallel reviewers, verify their findings, and act on them. Use as the author of a branch before opening its pull request (the implement-ticket skill points here) and at merge time, where the merge review runs it; both layers run the same protocol so the author's review asks the merge-time questions.
---

# Review a diff

One protocol for both layers. The **author** runs it on `git diff origin/main...HEAD` before opening the pull request. The **merge review** runs it at merge time through `scripts/merge-review.mjs` on every head whose CI passed, and again on each fix round's commits; an orchestration session runs it by hand when asked, on `git diff origin/main...origin/<branch>`. Reviewers get the diff, the branch's files, the issue text and the brief paths, never the author's summary or reasoning.

## Steps

1. **Pin the diff.** `git fetch origin`, then pick the checkout the reviewers read the branch's files from: the author's own worktree, the merge review's own checkout, or for a run by hand a detached worktree under the integration checkout's `.scratch/`, not the temp folder, which a headless reviewer cannot read (`git worktree add --detach <dir> origin/<branch>`), removed when the review is done. Write the diff command above to `.scratch/review.diff` in that checkout, where every reviewer can read it, and confirm it is non-empty. Never check out a branch in a shared checkout.
2. **Start the reviewers in parallel**, each in a fresh context. Write the issue text (`gh issue view <n>`) to `.scratch/issue.md` in the checkout; the reviewers are read-only and cannot fetch it. When the pull request names no issue, write its title alone (`gh pr view <n> --json title -q .title`): its description is the author's summary. Give each the diff file, the checkout's path, the issue file's path, the brief paths, and the report rule below.
   - **Correctness**, always: Claude Code's `/code-review` at `high`, or the `code-review:correctness-reviewer` agent.
   - **Spec**, always: the `spec-reviewer` agent.
   - **Surface**, always: the `surface-reviewer` agent.
   - **Failure**, when the diff touches anything outside the interface: the `failure-reviewer` agent.
   - **Security**, when the diff touches a process boundary, a pipe or socket, the file system, parsing of external input, credentials, or trust: the `code-review:security-reviewer` agent, told additionally: "Does the change widen what the user agreed to (a grant that outlives its revocation, a scope larger than the folder named), and does the interface say so where the user answers?" When that agent is not in the session, give the same questions and the report rule to a `general-purpose` agent, so the stage never silently does nothing.
   - **Design**, when the diff touches interface code: the `design-reviewer` agent.

   Size never exempts a diff: a 50-line, test-only change still gets Correctness, Spec and Surface, and Failure on the same terms as any other diff. What it touches does: a diff with no code and no tests (documents, briefs, skills) gets Spec and Surface only, and skips step 3.
3. **Mutate.** While they run, work in a second detached worktree of the same head (`git worktree add --detach <dir> <head>`), never in the reviewers' checkout: a reviewer reading a file mid-mutation sees code that is not the branch's. Remove it when done. When the issue names requirement IDs, run `node scripts/manifest.mjs --verifies <every ID written in full, comma-separated>` there; it must pass. Then, for every test that cites a requirement or covers a fix in the diff: revert the one wire the test depends on (flip the branch, drop the producer line), not the whole implementing hunk, which only proves the code is gone; run the citing test, watch it fail, and restore the wire. Then change each value the requirement names (a level, a code, a default) in the implementation, run the citing test, watch it fail, and restore the value. A test that stays green is a medium finding of its own ("passes without the change"), and so is a test that rewrites the expected value to match the output.
4. **Verify each finding** before acting: reproduce it with a test or a probe, or cite the line that shows it. A dismissal needs a reproduction or a cited line that shows the finding false; reasoning alone does not dismiss. Record every dismissal with its evidence. Rate by who gets hurt (ADR 0021): **high** is a wrong result, data loss or a security hole for Ersa's user or the owner, or unreviewed code reaching main; **medium** is a defect Ersa's user, the owner or the review loop meets in normal use, including owner-run tooling that fails silently, and a test that passes without the change counts as medium only when it cites a requirement or covers the fix of a high or medium finding; **low** is everything else, including owner-run tooling that fails loudly.
5. **Act.** Fix every confirmed finding of medium severity or above with the smallest change that removes the defect, with a test where one is cheap; fix low ones when trivial and list the rest as follow-ups. A finding whose fix needs a new mechanism (a cache, a background task, a protocol field, a state machine or reducer) becomes its own ticket unless the branch is unsafe to merge without it: new mechanisms bring their own defects. So does a reshape the Surface reviewer names that is larger than the ticket. A confirmed gap the brief does not cover gets an inbox line when the branch's code depends on it, and is a follow-up otherwise.
6. **Re-review the fixes.** After substantial fixes, run Correctness and Failure again on the fix commits alone; when a fix kept a new mechanism because the branch was unsafe without it, run them on the whole mechanism instead. A re-review in the same layer reads only the commits since the last review, and of a merge of main only its conflict resolution (ADR 0021). Fixes to concurrency, process-lifecycle and file-write findings are where new races come from.

## Report rule

Every reviewer's prompt ends with: "Read the diff's hunks and the functions they touch; follow a call only to confirm a finding. Report only findings you confirmed by reading the code or running the tests, each with file:line, severity (high, medium, low) and a one-line failure scenario. Say explicitly if you found nothing. Under 400 words."

## Common mistakes

- One combined reviewer in place of the parallel set: it misses what each specialist finds.
- A finding dismissed as "pre-existing" or "best effort" without a cited line: the dismissed case can be the very defect the ticket exists to fix.
- A fix round that adds a mechanism: the new mechanism draws findings of its own in the next round and is often removed again, so each round costs more than the smallest fix would have.
- A small diff reviewed by its author alone: a short or test-only diff still hides defects, including a build that misses the ticket's purpose.
- A known defect rated low because it needs a repeat or a slow peer: a leak or stall that needs a repeat still happens in use, and a shape already fixed elsewhere is not low here.
