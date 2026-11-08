# Review rubric

Every reviewer and the verifier of a pull request review follow this rubric, whether the review runs at merge time or by the author. The severities are ADR 0021's.

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

## What counts as verified

- Confirm a finding by reproducing it with a test or a probe in your own detached worktree, or by citing the line that shows it. Reasoning alone neither confirms nor dismisses.
- Dismiss a finding only with a reproduction or a cited line that shows it false, and record the evidence. "Pre-existing" and "best effort" are not evidence: the dismissed case can be the defect the ticket exists to fix.
- Report each confirmed finding once, with its location (`file:line`), severity, the defect and a one-line failure scenario.

## Scope

- A first review reads the whole change against main. A re-review reads only the commits since the last reviewed head, and of a merge of main only its conflict resolution. A mechanism a fix kept because the branch was unsafe without it is reviewed whole.
- Leave style alone. Formatting, naming taste and lint that CI already runs are not findings.
- A finding the review loop cannot act on, such as a defect on a planning-only path or a contradiction between harness rules, is still reported at its severity; say that it needs the owner.
