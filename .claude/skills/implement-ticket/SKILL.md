---
name: implement-ticket
description: Build one Ersa ticket end to end in an implementing session — read the briefs, write the code and tests, link related issues, run adversarial reviews of the change and fix what they confirm, run the fast local checks, then push and open the pull request. Use when a session is handed a ticket (an M0-Tn brief or similar) to implement.
---

# Implement a ticket

You are an implementing session. The ticket text you were given is the task; this skill is how you carry it to a pull request. The orchestration session reviews and merges after you; your job is to hand it a change that has already survived review.

## 1. Orient

- The ticket is a GitHub issue (ADR 0016). Read it in full, comments included: `gh issue view <n> --comments`. Check its blockers are closed (`docs/agents/issue-tracker.md`, **Count open blockers**); if one is open, stop and say so. If you were given the ticket as text instead, treat that text as the issue.
- Read `CLAUDE.md`, then every brief and ADR the ticket names. Read the `## Changes` section of each brief: recent decisions live there.
- Confirm you are in your own git worktree on a branch named for the ticket, never on `main`: `git status -sb`. If you are on `main` or in the shared checkout, stop and say so.
- Find other related open issues: `gh issue list --state open --search "<two or three key words>"`. Note any that the change also fixes or touches; they go in the pull request.

## 2. Build

- Work in small vertical slices: a failing test that cites the requirement (`// verifies: <ID>` in a test file), then the code that passes it. Use the `tdd` skill if it is installed.
- Plain, idiomatic code with the why commented: the owner is learning Rust.
- Where a brief is wrong, silent, or contradicts what you find, append one line to `docs/briefs/INBOX.md` (format in that file) and carry on with the most reasonable choice. Never edit `docs/briefs/` or `docs/adr/` otherwise.
- Commit as you go. Messages say what and why; commits that implement requirements end with `Implements: <IDs>`. No attribution trailers.

## 3. Review your own change adversarially

When the ticket's scope is built and `node scripts/check.mjs --quick` passes:

1. Get the diff: `git diff main...HEAD --stat` and the full diff.
2. Start reviewers **in parallel**, each a fresh subagent that gets the diff and the brief paths, **not** your reasoning or summary, so it cannot be anchored on your view. Give each one job:
   - **Correctness**: logic errors, edge cases, concurrency (ordering, cancellation, lock scope, things that can hang), error paths, resource leaks, behaviour on Windows. Use the `code-review:correctness-reviewer` agent type when available.
   - **Spec**: every requirement the ticket cites, checked against the brief text: is it implemented, is it tested by a test that would fail without it, and does anything contradict a brief or ADR?
   - **Security**, only if the change touches a process boundary, a socket or pipe, the file system, parsing of external input, or credentials: use `code-review:security-reviewer`.
   Ask each for at most ten findings, most severe first, each with file and line, a concrete failure scenario, and a fix.
3. **Verify before acting.** For each finding, check it against the code yourself, or with a test that reproduces it. A finding you cannot reproduce or ground in the code is dismissed, with a one-line reason.
4. Fix every confirmed finding of medium severity or above, with a test where one is cheap. Low findings: fix if trivial, otherwise list them as follow-ups.
5. If the fixes were substantial, run the correctness reviewer once more on the new diff.

## 4. Check, push, open the pull request

1. Run `node scripts/check.mjs`. Everything must pass; the manifest may warn only about inbox entries you added.
2. Push the branch: `git push -u origin HEAD`.
3. Open the pull request against `main` with `gh pr create`. Title: the ticket's title. Body sections, in order:
   - **What changed**: by area, with the decisions a later ticket must know.
   - **Requirements**: the IDs implemented and where each is verified.
   - **Review**: each confirmed finding and how it was fixed; dismissed findings with one-line reasons; follow-ups.
   - **Inbox entries** added, verbatim.
   - **Related issues**: `Fixes #<ticket>` for the ticket itself, then `Fixes #n` or `Refs #n` for any others.
   - **Checks**: what you ran locally, and anything you could not run (such as a live test).
4. End your turn with a short report: the pull request link, the check results, and anything the orchestration session must decide. Then stop. Do not push to the branch again unless asked; if asked, `git pull` first, since the orchestration session may have pushed a planning commit.
