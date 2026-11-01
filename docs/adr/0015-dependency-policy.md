# 0015: Gate third-party dependencies on known vulnerabilities and release age

- Status: accepted
- Date: 2026-11-01
- Related briefs: ARCH-CORE

## Context

Ersa pulls packages from crates.io now and from npm once the interface lands. Malicious or compromised releases on both registries are usually caught and yanked within days of publication, so a release that is days old is far likelier to be bad than one that is weeks old. Separately, vulnerabilities are disclosed against versions already in use, and need to be noticed whether or not anyone is touching dependencies. The owner asked for both checks in CI, with a way to make exceptions, notably to take a fresh release that fixes a vulnerability.

## Options considered

1. **Rely on Dependabot alerts only.** Catches known vulnerabilities after the fact; does nothing about very new releases and does not block a pull request.
2. **Per-ecosystem native tools only** (pnpm's minimum release age, `cargo audit`). The npm side has a native age gate, Cargo has none, and exceptions would live in several formats.
3. **CI gates with one policy and one exceptions file**: a vulnerability scan across every lockfile on every pull request and daily on `main`; a release-age check on packages a pull request adds or changes; native settings where an ecosystem has them, as a local first line.

## Decision

Option 3.

- **Vulnerabilities:** OSV-Scanner scans `Cargo.lock` and, later, `pnpm-lock.yaml` on every pull request and on a daily schedule against `main`. Any known vulnerability fails the check.
- **Release age:** a package version a pull request adds or changes in a lockfile must have been published at least **7 days** before the check runs. Versions already on `main` are not re-checked. Seven days covers the window in which bad releases are usually caught while keeping update lag small; it is one number in the policy file.
- **Sources and licences:** `cargo-deny` allows only crates.io as a source (no git or other registries) and a licence allowlist.
- **Exceptions:** one committed file, `deps-policy.toml`, holds the age threshold and both kinds of exception: a package version allowed in younger than the threshold (for example a release that fixes a vulnerability), and a vulnerability ID tolerated for now (for example when no fixed version exists yet). Every exception names a reason and an expiry date at most 90 days out; an expired exception fails the check, so exceptions are revisited rather than forgotten.
- **Interface packages** use pnpm, with `minimumReleaseAge` set to the same threshold, so a developer's local install obeys the policy before CI sees it.
- Dependabot opens version updates with the same cooldown; its security updates are not delayed.

## Consequences

Adding a dependency younger than a week needs an exception with a reason, which is the intended friction. A daily scan can turn `main` red because of a newly disclosed vulnerability in an unchanged dependency; that is the signal, and the fix is an update or an expiring exception. The age check needs the registry's publish dates at CI time, so a registry outage can fail it; the check reports that distinctly from a policy failure. Revisit the threshold if exceptions become routine.
