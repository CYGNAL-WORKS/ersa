# Review rubric

Every reviewer and the verifier of the merge review follow this rubric. The severities are ADR 0021's.

## Severity

Rate a finding by who gets hurt.

- **High:** a wrong result, data loss or a security hole for Ersa's user or the owner, or unreviewed code reaching main. Cleanup that can destroy something live is high.
- **Medium:** a defect Ersa's user, the owner or the review loop meets in normal use, including a gap in owner-run tooling that fails silently. A timeout around work that cannot be canceled, which leaves a thread or process behind on each expiry, is medium at least.
- **Low:** everything else, including a gap in owner-run tooling that fails loudly.

High and medium findings block the merge, whichever round found them. Low findings are reported and do not block.

Tests:

- A test that passes without the change is medium when it cites a requirement or covers the fix of a high or medium finding, and low otherwise.
- A test rewritten to expect whatever the code outputs is always medium.
- Mutation targets requirement values and fix hunks, not arbitrary lines.

A known defect stays at its severity when it needs a repeat or a slow peer to show: a leak or a stall that needs a repeat still happens in use. A shape already fixed elsewhere in the code is not low here.

## What the backend owns

Ersa abstracts what the backend CLIs do into one common shape; it never adds to or duplicates their concepts. If the backend owns it, Ersa asks the backend.

- A change that keeps its own copy of something a backend owns, or adds to a concept a backend owns, is medium at least. Report it even when a brief asks for it, and say that it needs the owner.
- Ersa's own concepts are outside this rule: a workspace, which Ersa makes for every backend even though a CLI may offer a worktree of its own, and the user's arrangement of sessions (ADR 0032).

## What counts as verified

- Confirm a finding by reproducing it with a test or a probe in your own detached worktree, or by citing the line that shows it. Reasoning alone neither confirms nor dismisses.
- Dismiss a finding only with a reproduction or a cited line that shows it false, and record the evidence. "Pre-existing" and "best effort" are not evidence: the dismissed case can be the defect the ticket exists to fix.
- Report each confirmed finding once, with its location (`file:line`), severity, the defect and a one-line failure scenario.

## Scope

- A first review reads the whole change against main. A re-review reads only the commits since the last reviewed head, and of a merge of main only its conflict resolution.
- Leave style alone. Formatting, naming taste and lint that CI already runs are not findings.
- A finding the review loop cannot act on, such as a defect on a planning-only path or a contradiction between harness rules, is still reported at its severity; say that it needs the owner.
