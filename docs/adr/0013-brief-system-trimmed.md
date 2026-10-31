# 0013: Briefs as source of truth, trimmed and enforced

- Status: accepted
- Date: 2026-10-31
- Supersedes: 0005

## Context

ADR 0005 established briefs with stable requirement IDs. An adversarial process review found the weight too high for a solo dogfood phase and several rules unenforceable: the write-back rule had no actor or trigger; "verified" counted any file containing a magic string; six brief types with a strict layering table were already contradicted by the first two briefs; thirty component briefs would duplicate what a component gallery and the design tokens already own; and prose rules in the constitution do not reliably bind coding agents under load.

## Options considered

1. **Keep 0005 as written**: rejected for the reasons above.
2. **Drop briefs; constitution, ADRs, glossary, and tests only**: rejected. Flows and protocols carry behaviour that code and tests do not express, and the interface needs an owner document for behaviour even if not for appearance.
3. **Trim to where stable IDs earn their keep, and replace honour-system rules with checks**.

## Decision

Option 3. The rules of 0005 stand except as amended here:

- **Requirement IDs live in flow and protocol briefs.** Product and architecture briefs carry requirements marked verified by review, not by test. Layout and component briefs carry no requirement IDs until the first dogfood milestone is complete, and stay `draft` until then.
- **Component briefs exist only for behaviour-heavy components** (for example the permission shelf, the composer, the rail tile) and contain purpose, behaviour, accessibility, and requirements. States, variants, and tokens are owned by the component gallery and the design tokens file.
- **Layering is "no cycles, no restating,"** checked by the manifest script. The type table is guidance, not a rule.
- **Write-back has an actor and a trigger.** Implementing sessions append findings to a committed inbox file. The manifest check fails while the inbox is non-empty, so a planning pass must empty it before the next merge.
- **Read-only briefs are enforced by a tool hook,** not a sentence: edits under the briefs and decisions directories are blocked unless the session declares the planning role.
- **"Verified" counts only citations in test files.** The manifest also reports duplicate requirement IDs and reference cycles as problems.
- **Commits that implement requirements carry an `Implements:` trailer** listing the IDs, so traceability survives ephemeral tickets.
- **The ADR index is generated** by the manifest script from the records' headers.

## Consequences

Fewer documents, each with a reason to exist. The manifest becomes a signal rather than a vanity metric. Planning passes have a concrete queue (the inbox) rather than a vague duty. The design system's visual truth lives in the gallery and tokens, where visual iteration is cheap, and briefs describe behaviour only.
