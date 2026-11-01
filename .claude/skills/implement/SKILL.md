---
name: implement
description: "Implement a piece of work based on a spec or set of tickets."
disable-model-invocation: true
---

> **Ersa adaptation.** This is the owner's interactive flow. Sessions the orchestrator starts use `implement-ticket` instead. Tickets are GitHub issues (`docs/agents/issue-tracker.md`). Commit before reviewing. "code-review" below is Claude Code's built-in `/code-review`; run it as `/code-review high` on the branch.

Implement the work described by the user in the spec or tickets.

If the user passes a ticket reference, fetch it from the issue tracker and state its title before starting. If the reference is ambiguous, ask.

Call the Skill tool with "tdd" where possible, at pre-agreed seams.

Run typechecking regularly, single test files regularly, and the full test suite once at the end.

Once done, call the Skill tool with "code-review" to review the work.

Commit your work to the current branch.
