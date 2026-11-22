# 0039: A finding the review loop cannot act on goes to a planning pass first

- Status: accepted
- Date: 2026-11-21
- Related briefs: none

## Context

ADR 0021 sends a finding the review loop cannot act on (one on a planning-only path, or a contradiction between harness rules) to the owner at once. Since then `CLAUDE.md` (Decide the details) has planning sessions settle a brief's details and take to the owner only a choice that departs from Claude Desktop or `PRD-ERSA`'s principles, is costly to reverse, or changes a decision the owner made, and the orchestrator runs a planning pass on a pull request's branch when its session asks for one (`docs/agents/orchestration.md`, Resolve inbox entries). In the 33 pull requests merged between #460 and #528, six such findings told the owner to decide; most were a detail of a brief or a design page, and a planning pass settled them.

## Options considered

1. **Keep sending them to the owner at once.** The owner sees every contradiction the review finds. The owner is asked about details `CLAUDE.md` gives to planning sessions, and the pull request waits on the answer.
2. **Send them to a planning pass first.** The implementing session asks the session that started it for a planning pass, which settles the finding as `CLAUDE.md` says and takes to the owner only what that rule leaves to them. The owner decides less and audits the calls in the briefs' Changes lines. A planning session can settle something the owner would have wanted to see.
3. **Let the implementing session change the planning-only path.** No wait at all. It ends the split between sessions that build and sessions that specify, which `CLAUDE.md`'s Roles rule exists to keep.

## Decision

Option 2. A finding the review loop cannot act on goes to a planning pass, and the implementing session does not try to fix it; the review says that it needs a planning pass, and that it needs the owner only when the call is one `CLAUDE.md` leaves to the owner. This replaces the last sentence of ADR 0021's decision; the rest of ADR 0021 stands. A change that keeps Ersa's own copy of something a backend owns still needs the owner (`REVIEW.md`, What the backend owns).

## Consequences

Such a finding is not an inbox entry, so the pull request waits on its open finding, not at the inbox gate, until the pass is pushed. The owner is asked less often and later: the call is in the brief's Changes line and in the merge report, where it can be overruled. A planning pass that changes the harness itself still reaches the owner, since those files are owner-only paths at the merge gate. Revisit if the owner overrules such calls often, or if planning passes on findings start to cost more review rounds than the questions they replace.
