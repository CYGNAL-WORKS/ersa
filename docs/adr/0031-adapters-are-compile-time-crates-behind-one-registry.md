# 0031: Adapters are compile-time crates behind one registry

- Status: accepted
- Date: 2026-11-09
- Related briefs: ARCH-CORE, PROTO-CORE-API

## Context

Ersa drives each backend through an adapter. The owner wants adapters to behave like plugins: a new one slots in by matching the adapter interface, and without one the app still compiles. Removing the Claude Code adapter is not planned; this is the shape the code should have. Today Core keeps its own list of backends, the binary names the Claude Code adapter in more than one place, and Core's tests need that adapter to compile. ARCH-CORE already rules out a plugin system for third-party adapters in the first milestones.

## Options considered

1. **Keep the current shape**: no work. Adding a backend keeps touching Core, and a backend that is registered but missing from Core's list cannot be chosen.
2. **Compile-time crates behind one registry**: each adapter is an in-tree crate, and the build lists the adapters it includes in one place. Simple and fully type-checked. Adding a backend means a rebuild.
3. **Load adapters at run time**: third parties could ship adapters without a rebuild. Rust has no stable binary interface, so this needs a process boundary or a C interface, plus versioning and trust rules for code Ersa did not build. Nothing needs it yet.

## Decision

Option 2. The registered adapters are the only backends Core knows, and Core names none of them. Each adapter says how its backend is detected and what limits it sets, recognizes its own recordings, and renders packs into its backend's native configuration.

A session whose backend has no registered adapter is tied to that adapter and cannot work without it, so it goes dormant rather than being deleted: Core does not load it, keeps what it stores about it, and shows it again once the adapter is back.

## Consequences

Adding a backend means one new crate and one registration, with no change to Core or the Interface, and a machine check holds that true. The order of registration decides the default backend. A workspace or project that holds a dormant session cannot be removed, because removal never cascades and going ahead would lose the session for good.

Revisit this if third parties need to ship adapters without rebuilding Ersa.
