# Dependencies

How Ersa's dependency policy runs. ADR 0015 is the decision and its reasons; `deps-policy.toml` holds the threshold and every exception.

## The checks

| Check | Runs | Fails when |
|---|---|---|
| Release age (`node scripts/deps-check.mjs age`) | every pull request | a package version the pull request adds or changes in any tracked `Cargo.lock` or `pnpm-lock.yaml` was published less than `min_age_days` (7) days ago, or does not come from crates.io or npm (a path outside the workspace, git, a tarball), and has no age exception |
| Vulnerabilities (`node scripts/deps-check.mjs vulns`) | every pull request and push to `main`, and daily on `main` | OSV-Scanner finds a known vulnerability in any lockfile that has no vulnerability exception |
| Sources, bans, licences (`cargo deny --locked check bans licenses sources`) | every pull request and push to `main`, and daily on `main` | a crate comes from anywhere but crates.io, a dependency uses a `*` version, or a licence is not on the allowlist in `deny.toml` |

Both script modes also fail when `deps-policy.toml` has an exception that has expired, lacks a reason or expiry, or expires more than 90 days after the day the check runs.

The script exits 1 for a policy failure and 2 when it could not run: a registry did not answer, OSV-Scanner or git failed, or a lockfile is in a format it does not read. Re-run a job that exits 2 after fixing what it names; it says nothing about the dependencies.

CI builds with `--locked`, so it never resolves versions `Cargo.lock` does not already name. Dependabot proposes weekly updates with a 7-day cooldown on the packages it chooses to update; its security updates are not delayed. An update can also move transitive packages to versions younger than seven days, and then the age check fails the Dependabot pull request like any other: wait and re-run, or add an exception.

To run the checks locally, install [OSV-Scanner](https://github.com/google/osv-scanner) and `cargo-deny`, fetch `origin`, and run the three commands above. `age` compares with `origin/main` unless given `--base <ref>`. `vulns` finds OSV-Scanner on `PATH`, or at the path in `OSV_SCANNER`.

## Adding an exception

Exceptions go only in `deps-policy.toml`, never in an `osv-scanner.toml` or `deny.toml`, so there is one place to review them. Each needs a `reason` that says why accepting it is safe and what ends it, and an `expires` date (YYYY-MM-DD, unquoted) at most 90 days out. Say why in the pull request too.

To take a package version younger than seven days, typically a release that fixes a vulnerability:

```toml
[[age_exception]]
ecosystem = "crates.io"   # or "npm"
package = "example-crate"
version = "1.4.2"         # exact, as in the lockfile
reason = "Fixes the advisory against 1.4.1."
expires = 2026-12-01
```

Remove it once the version is a week old; it is no longer needed. A package that is not from the registry has no publish date, so it needs an age exception for as long as it stays; its `version` is the text the lockfile shows, such as `file:../local-thing`.

To tolerate a known vulnerability for now, typically when no fixed version exists or Ersa cannot reach the affected code:

```toml
[[vulnerability_exception]]
id = "RUSTSEC-2024-0001"  # the RUSTSEC, GHSA, or CVE ID; any alias matches
package = "example-crate" # optional: only this package
version = "1.4.1"         # optional, needs package: only this version
reason = "No fixed release yet; Ersa never calls the affected function."
expires = 2026-12-01
```

Prefer setting `package` (and `version`), so the exception cannot also silence the same advisory on another package.

To allow a new licence, add it to `allow` in `deny.toml` and explain the choice in the pull request; that is a policy change, not an exception.

## When the daily scan goes red

A new advisory was published against a version already on `main`, or an exception expired.

1. Read the job log: it names the package, version, and advisory IDs, or the exception.
2. If a fixed version exists, update to it in a pull request. If the fix is younger than seven days, add an age exception for it in the same pull request.
3. If no fix exists, add a vulnerability exception with a short expiry and a reason saying whether Ersa is affected, and keep an eye on the advisory.
4. For an expired exception, remove it if the dependency has been fixed, or renew it with a new reason and expiry.

`main` stays red until one of these lands; that is the signal working.
