# 0005: Briefs are the source of truth, with stable requirement IDs

- Status: superseded by 0013
- Date: 2026-10-31

## Context

The code is mutable and mostly agent-written. We need a durable description of what Ersa does that survives rewrites and lets a fresh session rebuild a module to the same behaviour. Spec-as-source (humans edit only the spec, code is regenerated) is a philosophical north star but not practical today: regeneration is non-deterministic and loses implementation decisions. The predecessor used briefs successfully but suffered rewrite loops: a design change forced a brief rewrite, which forced test rewrites, with references breaking along the way.

## Options considered

1. **No specs; constitution, ADRs, and tests only**: lightest. Rejected for a product this large: behaviour would live only in code and tests, and the interface would have no owner document.
2. **Adopt a spec framework (spec-kit, Kiro, OpenSpec)**: tooling for free. Rejected: they are built around per-change specs on branches (spec-first, not spec-anchored), are verbose to review, and take control of the workflow.
3. **Spec-anchored briefs of our own, with stable IDs**: one brief per capability, kept alive after shipping; requirements carry IDs that tests cite; pointers run one way only.

## Decision

Option 3, with these rules:

- Briefs describe behaviour from the user's perspective and contain no file paths or code. Protocol briefs may contain schemas, since there the schema is the behaviour.
- Brief types are product, flow, layout, component, protocol, architecture. A brief references lower layers by ID and never restates their content.
- Each requirement has a stable ID. Tests cite IDs with `verifies: <ID>`. Rewording keeps the ID; a change of meaning retires it and issues a new one. IDs are never reused.
- Briefs are elaborated just in time: a `draft` brief is a paragraph and open questions, fully written only when next to be built.
- Implementing sessions treat briefs as read-only and report contradictions instead of fixing them.
- Plans and tickets are ephemeral; decisions made during implementation are written back before a ticket closes.
- The manifest is generated, never edited.

## Consequences

A design change touches one brief and the tests whose cited IDs changed meaning, and nothing else. Visual changes land in design tokens and screenshot approvals, not in briefs. We accept the cost of maintaining roughly thirty component briefs and a handful of layout and flow briefs. Rebuildability is tested occasionally by having a fresh agent rebuild one module from its briefs alone, not by regenerating the application.
