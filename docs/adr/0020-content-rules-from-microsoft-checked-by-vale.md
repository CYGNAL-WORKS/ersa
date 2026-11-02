# 0020: Content rules come from the Microsoft Writing Style Guide, checked by Vale, with interface copy in a string catalog

- Status: accepted
- Date: 2026-11-02
- Related briefs: PRD-ERSA

## Context

Ersa had no written content rules and nothing that checked its wording, and the interface's copy is written inline in its components, where no tool can read it. The repository mixes British and US spelling. Ersa is Windows first, so its users read Microsoft's wording all day. Translation is a likely later need.

## Options considered

1. **Keep a prose paragraph**: no setup. Unenforced, so it drifts with every session that writes copy.
2. **Adopt the Microsoft guide whole, as a fallback for anything our rules miss**: complete and familiar. Its word lists ban "backend" and rename "agent", which are Ersa's own terms, and "follow the fallback" cannot be checked.
3. **Google's developer documentation guide**: strong for docs, thin on interface copy.
4. **Take rules from the Microsoft guide one by one, write them down, and check them with Vale in CI; move interface copy into a string catalog that Vale and a test can read**: every rule is explicit and most are checked. Costs a Vale setup, an `Ersa` style built from the glossary, and moving every interface string.

## Decision

Option 4, in US English. `docs/design/CONTENT.md` holds the rules and names the check for each; the Microsoft package supplies most rules, an `Ersa` style supplies the rest, and the string catalog holds every string the Interface shows.

## Consequences

Copy can be reviewed by a machine, and the catalog is ready for other locales. Existing docs and code are converted to US spelling once (accepted ADRs excepted, as records), and every interface string moves into the catalog. Vale is a tool outside npm and crates.io, so CI pins its release and the Microsoft package's, and treats them under ADR 0015's age rule by hand. Revisit if Vale's false positives cost more than the drift they stop.
