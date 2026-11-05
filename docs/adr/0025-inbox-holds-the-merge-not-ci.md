# 0025: An open inbox entry holds the merge at the gate, not CI

- Status: accepted
- Date: 2026-11-05
- Related briefs: none

## Context

ADR 0013 gave write-back an actor and a trigger: implementing sessions append brief gaps to the inbox, and the manifest check fails while it has open entries, so a planning pass fixes the brief before the next merge. That works when used: all 24 pull requests that added an entry got their brief fixed on the branch before merge. But since ADR 0023 the merge review runs only on a head whose CI passed, so an honest entry also stops the review, and `implement-ticket` told the author both that the manifest may fail on its entries and to push until CI is green. Three pull requests (#111, #118, #135) kept their gaps out of the file to stay green; two of the three gaps were lost. Harness and process proposals no branch depends on were also written as entries and blocked merges for nothing.

## Options considered

1. **Keep the check in CI and fix the skill's wording.** Smallest change. An honest entry still stops the review, so the reason to dodge stays.
2. **Retire the inbox; each gap becomes an issue and nothing blocks.** Nothing to dodge. Code merges ahead of its brief, the next session reads the wrong brief, and the gaps that skipped the block are the ones that were lost.
3. **Route gaps through the merge review as a new finding kind.** Rebuilds what the gate's pending path, the Spec reviewer and the manifest already do as model judgment, and moves detection from a grep to a model.
4. **Move the block from CI to the merge gate.** The manifest stops counting open entries as a problem, so CI stays green and the review runs; the gate refuses a head whose inbox has an open entry, and the head waits in the gate's existing pending path until a planning commit closes it.

## Decision

Option 4. The manifest check still fails on every other problem, in CI and on the merge queue's merges, and on the merge queue's merges and on main it also fails on an open entry, so a merge that skips the gate, such as the owner's, cannot carry one onto main. The merge review runs the gate for the owner's pull requests too, so one with an open entry shows as waiting rather than ready. The inbox holds only gaps in a brief, `CONTENT.md` or `DESIGN.md` that the branch's code depends on. A harness or process proposal no branch depends on is not an entry: an attended session proposes it in chat, and an unattended one lists it as a follow-up in its pull request, which the orchestration session files as a `needs-triage` issue.

## Consequences

ADR 0013's intent holds: no code merges ahead of the brief it depends on, and the trigger is now the gate rather than CI. CI turns red only on something the author can fix. The local check and the commit hook lose their inbox exceptions. A head with an open entry shows a pending merge review, not a red one, which the implementing session reports as waiting on planning. Revisit if entries sit unresolved for long enough to hold pull requests up.
