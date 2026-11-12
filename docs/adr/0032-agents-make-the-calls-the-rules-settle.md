# 0032: Agents make the calls the rules settle

- Status: accepted
- Date: 2026-11-12
- Related briefs: none

## Context

Orchestrated sessions took product and design questions to the owner whenever a brief left them open: what a fork is named, whether a reviewer's finding about a defect the ticket did not cause should block, whether a brief that contradicts a harness rule wins. Most of these had one answer once the owner's standing rules were applied: match what the vendor's CLI or Claude Desktop does, and let the backend own what it owns. The owner answered the same kind of question again and again, and pull requests waited on those answers.

## Options considered

1. **Keep escalating open questions.** The owner sees every call. Work waits on the owner, and the answers repeat.
2. **Agents decide everything.** Nothing waits. Calls that are costly to reverse, or that change direction, are made without the person who sets it.
3. **Agents decide by a fixed order of rules, record each call, and escalate only what the order leaves open.** Most questions settle without waiting, and every call is visible and can be overruled.

## Decision

Option 3. `CLAUDE.md` gives the order: a brief or ADR; what the vendor's CLI or Claude Desktop does; what the backend owns; `DESIGN.md` and `CONTENT.md`; else the choice that is cheapest to change later. The owner still decides a call that would need an ADR, changes a ticket's scope, sets two of these rules against each other, or is hard to undo outside the repository. The merge review stops blocking on a defect the change neither caused nor was meant to fix: it becomes a follow-up. Findings that need a brief changed go to a planning pass, not to the owner.

## Consequences

The owner learns of most calls after they are made, in each merge report, and overrules one by saying so; a call overruled more than once becomes a rule here or in a brief. A defect found outside a ticket's scope ships in the merge it was found on and waits for its own ticket, so follow-ups must not be lost (the harness pass files them). Docket's round limit still sends a pull request to the owner after three rounds. Revisit if overruled calls grow common, or if follow-up defects reach users before their tickets do.
