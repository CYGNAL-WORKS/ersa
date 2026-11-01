# 0016: Tickets are GitHub issues

- Status: accepted
- Date: 2026-11-01
- Related briefs: none

## Context

Tickets were written as uncommitted files in the orchestration session's `.scratch/` and pasted into each implementing session's prompt. The owner could not see the queue without asking, nothing linked a pull request back to the work it closed, and blocking order lived only in the orchestrator's head. The owner wants the issue tracker used, and the skills adopted from `mattpocock/skills` (`to-tickets`, `implement-ticket`'s issue linking) assume one.

## Options considered

1. **Keep tickets in `.scratch/`.** Private to one checkout, invisible to the owner, no links.
2. **Commit tickets as files.** Visible, but they outlive the work and turn into a second, stale specification beside the briefs.
3. **GitHub issues.** Visible on a board, linked from pull requests (`Fixes #n`), closed when the work merges, with native blocking links between tickets.

## Decision

Option 3. Tickets are GitHub issues in `KPandaK/ersa`, cut from briefs with the `to-tickets` skill, conventions in `docs/agents/issue-tracker.md`. A ticket names the brief and requirement IDs it implements, the area it touches, and what done looks like; it carries the `ready-for-agent` label when it can be picked up and blocking links to the tickets that gate it. The pull request that completes it says `Fixes #n`. Briefs stay the source of truth for behaviour: an issue never restates a brief, it points at one.

## Consequences

The owner sees the queue and its order on GitHub. Tickets remain ephemeral in the sense that matters: they close when their pull request merges and are not edited afterwards. Plans that are not tickets (an orchestrator's working notes) may still live in `.scratch/`. Traceability still rests on requirement IDs in tests and `Implements:` trailers, not on issue numbers.
