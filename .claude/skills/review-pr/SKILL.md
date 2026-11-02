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

   Size never exempts a diff: a 50-line, test-only change still gets Correctness and Spec, and Failure when it runs a child process.
3. **Mutate.** While they run, `node scripts/manifest.mjs --verifies <the issue's requirement IDs, comma-separated>` must pass. Then, for every test that cites a requirement or covers a fix in the diff: revert the implementing hunk, run the citing test, and watch it fail; restore the hunk. Change each value the requirement names (a level, a code, a default) the same way. A test that stays green is a medium finding of its own ("passes without the change"), and so is a test that rewrites the expected value to match the output.
4. **Verify each finding** before acting: reproduce it with a test or a probe, or cite the line that shows it. A dismissal needs a reproduction or a cited line that shows the finding false; reasoning alone does not dismiss. Record every dismissal with its evidence.
5. **Act.** Fix every confirmed finding of medium severity or above with the smallest change that removes the defect, with a test where one is cheap; fix low ones when trivial and list the rest as follow-ups. A finding whose fix needs a new mechanism (a cache, a background task, a protocol field, a state machine or reducer) becomes its own ticket unless the branch is unsafe to merge without it: new mechanisms bring their own defects. A confirmed gap the brief does not cover gets an inbox line.
6. **Re-review the fixes.** After substantial fixes, run Correctness and Failure again on the fix commits alone. Fixes to concurrency, process-lifecycle and file-write findings are where new races come from.

## Reviewer briefs

Every brief ends with: "Read the diff's hunks and the functions they touch; follow a call only to confirm a finding. Report only findings you confirmed by reading the code or running the tests, each with file:line, severity (high, medium, low) and a one-line failure scenario. Say explicitly if you found nothing. Under 400 words."

**Spec.** "Compare the diff with the issue and every brief and requirement ID it cites. Report (a) requirements missing or partial, (b) behavior nobody asked for, (c) requirements that look implemented but wrong, including a value shown or sent from a different source than the one the requirement names, (d) requirements without a test that would fail without the change, and (e) fields, ids or codes the tests assert that no brief names. Quote the brief line for each."

**Failure.** "Review this diff against peers Ersa does not control: child processes, pipes, sockets, the database, the file system, vendor CLIs, git, the operating system's process table. Assume each peer can be wedged, slow, hostile, newer, localized or gone, that a pid or a file can be reused or replaced between two steps, and that Core can crash at any line. Per hunk ask: does stopping or exit detection depend on EOF, on a bare pid, or on a child that ends? Is a lock held across a blocking call or an await? Is untrusted input bounded? Does a parse failure surface as an event or become a silent default? Can a write leave a torn or duplicated line, and does the next start repair it? Who else can open or impersonate this endpoint? If a test panics or times out at any line, does a process it started outlive the test run? Does any branch match human-readable text? A timeout around work that cannot be cancelled (`spawn_blocking`, a blocking system call, a child nobody kills) ends the wait, not the work: does each expiry leave a thread or process behind that repeats can pile up? That is medium at least. When a step fails or times out, what has already happened? A peer slower than the timeout is still working, and an error that arrives after a message was delivered leaves its effect in place: cleanup that can destroy something live is high. Does a child inherit environment (`CARGO_TARGET_DIR`, `PATH`, `ERSA_*`) that changes what it runs or where it writes, and do its errors still reach the log or the test output? Report hunk, trigger, consequence."

**Security.** The `code-review:security-reviewer` agent, told additionally: "Does the change widen what the user agreed to (a grant that outlives its revocation, a scope larger than the folder named), and does the interface say so where the user answers?"

**Design.** "Check the interface changes against `docs/design/DESIGN.md` and the Design section of `CLAUDE.md`, both themes, every state. Every color, size, radius, duration and easing is a token. Hover, focus, pressed, open, disabled, loading, empty and error each look deliberate and distinct, and an open state wins over hover. For every transition (open, close, save, cancel, failure, disconnect, remount) say where focus lands and that it is visible; Escape closes and returns focus. Nothing jumps: an element that becomes editable keeps its size and position, and one that arrives late (a heading, a group name) holds its space or enters with a motion token. Every container has its padding token; compare each screen's PNG with its neighbors. Motion uses a motion token and honors reduced motion. Copy is plain, agrees in number, and is true in the state it labels (no 'New' on something that exists). Gallery baselines exist for each state in both themes; open the PNGs and look."

## Common mistakes

- One combined reviewer in place of the parallel set: it misses what each specialist finds.
- A finding dismissed as "pre-existing" or "best effort" without a cited line: on #48 the dismissed case was the exact leak the ticket fixed.
- A fix round that adds a mechanism: #57's round-one truncate drew a round-two finding and was removed in round three; #71's round-one stage reducer drew a high and then a medium.
- A small diff reviewed by its author alone: #68 skipped the reviewers as 60 test-only lines, and merge review found three defects, among them a build that ignored the ticket's purpose.
- A known defect rated low because it needs a repeat or a slow peer: #70 listed its leaked blocking thread as a low follow-up, the same shape #58 had just fixed.
