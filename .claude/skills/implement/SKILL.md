---
name: implement
description: "Implement a piece of work based on a spec or set of tickets."
disable-model-invocation: true
---

> **Ersa adaptation.** Tickets are GitHub issues (`docs/agents/issue-tracker.md`); read the issue with its comments and stop if a blocker is still open. Work only in your own git worktree on a branch for the ticket, never `main`. Read `CLAUDE.md` and every brief and ADR the ticket names, including each brief's `## Changes`. Tests cite requirement IDs with `verifies: <ID>`; commits that implement requirements end with `Implements: <IDs>`. Where a brief is wrong or silent, add a line to `docs/briefs/INBOX.md` and carry on. The checks are `node scripts/check.mjs --quick` while working and `node scripts/check.mjs` at the end. Run `code-review` with `main` as the fixed point and fix what it confirms. Then push, open a pull request against `main` written with the `pr` skill, and end your turn with the link.

Implement the work described by the user in the spec or tickets.

If the user passes a ticket reference, fetch it from the issue tracker and state its title before starting. If the reference is ambiguous, ask.

Call the Skill tool with "tdd" where possible, at pre-agreed seams.

Run typechecking regularly, single test files regularly, and the full test suite once at the end.

Once done, call the Skill tool with "code-review" to review the work.

Commit your work to the current branch.
