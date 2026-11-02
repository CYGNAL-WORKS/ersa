---
name: review-pr
description: Review a diff adversarially with parallel reviewers, verify their findings, and act on them. Use as the author of a branch before opening its pull request (the implement-ticket skill points here) and as the orchestration session at merge time; both layers run the same protocol so the author's review asks the merge-time questions.
---

# Review a diff

One protocol for both layers. The **author** runs it on `git diff origin/main...HEAD` before opening the pull request. The **orchestrator** runs it on `git diff origin/main...origin/<branch>` at merge time, and again on each fix round's commits. Reviewers get the diff command, the issue number and the brief paths, never the author's summary or reasoning.

## Steps

1. **Pin the diff.** `git fetch origin`, then the diff command above; confirm it is non-empty. Reviewers read from `origin/<branch>` and never check out a branch in a shared checkout: one that must run tests makes its own detached worktree under the scratchpad (`git worktree add --detach <dir> origin/<branch>`) and removes it when done.
2. **Start the reviewers in parallel**, each in a fresh context, with the brief from the reference below, the diff command, the issue number and the brief paths:
   - **Correctness**, always: Claude Code's `/code-review` at `high`, or the `code-review:correctness-reviewer` agent.
   - **Spec**, always.
   - **Failure**, when the diff touches anything outside the interface.
   - **Security**, when the diff touches a process boundary, a pipe or socket, the file system, parsing of external input, credentials, or trust.
   - **Design**, when the diff touches interface code.
3. **Mutate.** While they run, for every test that cites a requirement or covers a fix in the diff: revert the implementing hunk, run the citing test, and watch it fail; restore the hunk. A test that stays green is a medium finding of its own ("passes without the change"), and so is a test that rewrites the expected value to match the output.
4. **Verify each finding** before acting: reproduce it with a test or a probe, or cite the line that shows it. A dismissal needs a reproduction or a cited line that shows the finding false; reasoning alone does not dismiss. Record every dismissal with its evidence.
5. **Act.** Fix every confirmed finding of medium severity or above with the smallest change that removes the defect, with a test where one is cheap; fix low ones when trivial and list the rest as follow-ups. A finding whose fix needs a new mechanism (a cache, a background task, a protocol field) becomes its own ticket unless the branch is unsafe to merge without it: new mechanisms bring their own defects. A confirmed gap the brief does not cover gets an inbox line.
6. **Re-review the fixes.** After substantial fixes, run Correctness and Failure again on the fix commits alone. Fixes to concurrency, process-lifecycle and file-write findings are where new races come from.

## Reviewer briefs

Every brief ends with: "Read the diff's hunks and the functions they touch; follow a call only to confirm a finding. Report only findings you confirmed by reading the code or running the tests, each with file:line, severity (high, medium, low) and a one-line failure scenario. Say explicitly if you found nothing. Under 400 words."

**Spec.** "Compare the diff with the issue and every brief and requirement ID it cites. Report (a) requirements missing or partial, (b) behaviour nobody asked for, (c) requirements that look implemented but wrong, (d) requirements without a test that would fail without the change, and (e) fields, ids or codes the tests assert that no brief names. Quote the brief line for each."

**Failure.** "Review this diff against peers Ersa does not control: child processes, pipes, sockets, the database, the file system, vendor CLIs, git, the operating system's process table. Assume each peer can be wedged, slow, hostile, newer, localized or gone, that a pid or a file can be reused or replaced between two steps, and that Core can crash at any line. Per hunk ask: does stopping or exit detection depend on EOF, on a bare pid, or on a child that ends? Is a lock held across a blocking call or an await? Is untrusted input bounded? Does a parse failure surface as an event or become a silent default? Can a write leave a torn or duplicated line, and does the next start repair it? Who else can open or impersonate this endpoint? If a test panics or times out at any line, does a process it started outlive the test run? Does any branch match human-readable text? Report hunk, trigger, consequence."

**Security.** The `code-review:security-reviewer` agent, told additionally: "Does the change widen what the user agreed to (a grant that outlives its revocation, a scope larger than the folder named), and does the interface say so where the user answers?"

**Design.** "Check the interface changes against `docs/design/DESIGN.md` and the Design section of `CLAUDE.md`, both themes, every state. Every colour, size, radius, duration and easing is a token. Hover, focus, pressed, open, disabled, loading, empty and error each look deliberate and distinct, and an open state wins over hover. For every transition (open, close, save, cancel, failure, disconnect) say where focus lands and that it is visible; Escape closes and returns focus. Nothing jumps: an element that becomes editable keeps its size and position. Motion uses a motion token and honours reduced motion. Copy is plain and agrees in number. Gallery baselines exist for each state in both themes; open the PNGs and look."

## Common mistakes

- One combined reviewer in place of the parallel set: it misses what each specialist finds.
- A finding dismissed as "pre-existing" or "best effort" without a cited line: on #48 the dismissed case was the exact leak the ticket fixed.
- A fix round that adds a mechanism: #57's round-one truncate drew a round-two finding and was removed in round three.
