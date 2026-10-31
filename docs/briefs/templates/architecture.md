---
id: ARCH-EXAMPLE
type: architecture
title: Noun phrase naming the system or subsystem
status: draft
references: []
adrs: []
---

# Title

## Context

What this system is, who and what it talks to. A Mermaid diagram at the "system context" level: this system, its users, and external systems.

## Parts

One subsection per part (container, crate, service). For each: responsibility in one sentence, what it owns, what it must not do, and which protocols it speaks. Name parts with glossary terms, not file paths.

## Boundaries

The rules that keep parts honest: what may depend on what, what crosses each boundary and in which form.

## Cross-cutting concerns

Persistence, error handling, logging, security, configuration. One short paragraph each.

## Implementation decisions

Choices that an implementer needs and that would otherwise be made inconsistently: runtimes, storage engines, serialization, concurrency model. Record the why in an ADR and link it; state the what here.

## Milestones

Ordered, each with a one-line definition of done. The first milestone the user can live in daily is marked.

## Requirements

- **ARCH-EXAMPLE-R01**: ...

## Out of scope

## Open questions

## Changes

- YYYY-MM-DD: created.
