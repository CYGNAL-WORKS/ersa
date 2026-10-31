# 0001: Record decisions as ADRs

- Status: accepted
- Date: 2026-10-31

## Context

Ersa is built largely by agents across many sessions. Each session starts without the memory of why earlier choices were made. Without a record of the why, settled questions get re-litigated and documents get "fixed" back to rejected options. The project this one replaces suffered exactly this churn.

## Options considered

1. **Decisions inline in briefs**: one place to look. Briefs churn with product changes, so rationale gets lost or bloats them.
2. **Decisions in commit messages and PRs**: zero overhead. Not discoverable by an agent planning a change; scattered.
3. **Lightweight ADRs**: one page per decision, MADR-style, superseded rather than edited. Discoverable, stable, cheap; a small discipline cost.

## Decision

Option 3. One decision per file under `docs/adr/`, following `0000-template.md`. Accepted records are not edited except for status.

## Consequences

Planning sessions read the ADR index before proposing a change in direction. Design decisions about the interface are recorded here too, not only technical ones. The index in `README.md` is maintained by hand and is the one place this directory can drift; keep it short.
