#!/usr/bin/env node
// Enforces the dependency policy of ADR 0015 against the lockfiles.
//
//   node scripts/deps-check.mjs age [--base <ref>]   release age of what changed since <ref>
//   node scripts/deps-check.mjs vulns                 known vulnerabilities in every lockfile
//
// Lockfiles are every Cargo.lock and pnpm-lock.yaml git tracks, at any depth.
// `age` compares them on disk with the same paths at <ref> (default origin/main)
// and looks up the publish time of every package version added or changed.
// `vulns` runs OSV-Scanner (on PATH, or the path in OSV_SCANNER).
// Both read deps-policy.toml for the threshold and the exceptions.
//
// Exit status: 0 pass, 1 policy failure, 2 the check could not run
// (registry, scanner, git, or lockfile error). docs/dependencies.md explains
// what to do.

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const POLICY = "deps-policy.toml";
const ECOSYSTEMS = { "Cargo.lock": "crates.io", "pnpm-lock.yaml": "npm" };
const MAX_BUFFER = 256 * 1024 * 1024;
const CRATES_IO_SOURCES = new Set([
  "registry+https://github.com/rust-lang/crates.io-index",
  "sparse+https://index.crates.io/",
]);
const USER_AGENT = "ersa-deps-check (https://github.com/CYGNAL-WORKS/ersa)";
const MAX_EXCEPTION_DAYS = 90;
const DAY_MS = 86_400_000;

// ---- Lockfiles ---------------------------------------------------------------

// Every [[package]] in a Cargo.lock as { name, version, source }. Workspace
// crates have no `source`.
export function parseCargoLock(text) {
  const packages = [];
  let current = null;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line === "[[package]]") {
      current = {};
      packages.push(current);
    } else if (line.startsWith("[")) {
      current = null;
    } else if (current) {
      const kv = line.match(/^(name|version|source)\s*=\s*"([^"]*)"$/);
      if (kv) current[kv[1]] = kv[2];
    }
  }
  return packages
    .filter((p) => p.name && p.version)
    .map((p) => ({ name: p.name, version: p.version, source: p.source ?? null }));
}

// Every package in a pnpm-lock.yaml (v6 and later) as { name, version, source }. Keys come from the top-level `packages:` map,
// such as `lodash@4.17.21`, `'@types/node@22.1.0'`, or (v6)
// `/react-dom@18.2.0(react@18.2.0)`. Versions that are not plain semver
// (`file:`, `link:`, tarball and git URLs) are not from the registry; their
// source is the version text.
export function parsePnpmLock(text) {
  const packages = new Map();
  let inPackages = false;
  for (const line of text.split(/\r?\n/)) {
    if (/^\S/.test(line)) {
      inPackages = /^packages:\s*$/.test(line);
      continue;
    }
    if (!inPackages) continue;
    const m = line.match(/^ {2}(?! )(['"]?)(.+?)\1:(\s|$)/);
    if (!m) continue;
    let key = m[2].replace(/^\//, "");
    const peer = key.indexOf("(");
    if (peer > 0) key = key.slice(0, peer);
    const at = key.indexOf("@", 1);
    if (at < 0) continue;
    const name = key.slice(0, at);
    const version = key.slice(at + 1);
    const registry = /^\d+\.\d+\.\d+([-+][0-9A-Za-z.+-]*)?$/.test(version);
    packages.set(`${name}@${version}`, { name, version, source: registry ? "npm" : version });
  }
  return [...packages.values()];
}

const key = (p) => `${p.name}@${p.version}`;

// Packages in `head` whose exact name@version is not in `base`: added crates and
// new versions of existing ones. Versions already on the base are not re-judged.
export function diffPackages(base, head) {
  const before = new Set(base.map(key));
  const seen = new Set();
  return head.filter((p) => {
    const k = key(p);
    if (before.has(k) || seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

// The lockfiles among `git ls-files` output, as { path, ecosystem }.
export function lockfilesFrom(lsFiles) {
  return lsFiles
    .split(/\r?\n/)
    .filter((path) => posix.basename(path) in ECOSYSTEMS)
    .map((path) => ({ path, ecosystem: ECOSYSTEMS[posix.basename(path)] }));
}

// The lockfile can't be judged: the check could not run, which is not a policy failure.
export class CheckError extends Error {}

// Parses one lockfile. Missing or empty text is no packages; text that yields
// no packages is a format this script does not read, which must not pass as
// "nothing changed".
export function parseLockfile(ecosystem, text, label) {
  if (!text.trim()) return [];
  if (ecosystem === "npm") {
    const version = text.match(/^lockfileVersion:\s*['"]?(\d+)/m);
    if (!version || Number(version[1]) < 6) {
      throw new CheckError(`${label}: lockfileVersion ${version?.[1] ?? "?"} is not supported; use pnpm 8 or later (lockfile v6+)`);
    }
  }
  const packages = ecosystem === "npm" ? parsePnpmLock(text) : parseCargoLock(text);
  if (!packages.length) throw new CheckError(`${label}: found no packages in a non-empty lockfile; its format is not one this script reads`);
  return packages;
}

// Where a package version comes from, for the age check: "workspace" (one of
// the repository's own crates, a member of any of its Cargo workspaces, listed
// in `workspaceMembers` as name@version),
// "registry" (crates.io or npm, so it has a publish time), or "unregistered"
// (anything else: a path outside the repository's workspaces, git, a tarball, another
// registry), which has no publish time to judge.
export function classifyPackage(ecosystem, pkg, workspaceMembers) {
  if (ecosystem === "npm") return pkg.source === "npm" ? "registry" : "unregistered";
  if (pkg.source === null) return workspaceMembers.has(key(pkg)) ? "workspace" : "unregistered";
  return CRATES_IO_SOURCES.has(pkg.source) ? "registry" : "unregistered";
}

// ---- Policy file -------------------------------------------------------------

// The subset of TOML deps-policy.toml uses: comments, `key = value` with string,
// integer, or bare-date values, and [[array-of-tables]] headers.
export function parsePolicyToml(text) {
  const doc = {};
  let table = doc;
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = stripComment(lines[i]).trim();
    if (!line) continue;
    const where = `${POLICY} line ${i + 1}`;
    const header = line.match(/^\[\[\s*([A-Za-z0-9_-]+)\s*\]\]$/);
    if (header) {
      table = {};
      (doc[header[1]] ??= []).push(table);
      continue;
    }
    const kv = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(.+)$/);
    if (!kv) throw new Error(`${where}: cannot read "${line}"`);
    const [, name, rawValue] = kv;
    if (name in table) throw new Error(`${where}: "${name}" is set twice`);
    table[name] = parseValue(rawValue.trim(), where);
  }
  return doc;
}

function stripComment(line) {
  let quoted = false;
  for (let i = 0; i < line.length; i++) {
    if (line[i] === '"' && line[i - 1] !== "\\") quoted = !quoted;
    else if (line[i] === "#" && !quoted) return line.slice(0, i);
  }
  return line;
}

function parseValue(v, where) {
  if (/^"([^"\\]|\\.)*"$/.test(v)) return JSON.parse(v);
  if (/^-?\d+$/.test(v)) return Number(v);
  if (/^\d{4}-\d{2}-\d{2}$/.test(v)) return { date: v };
  throw new Error(`${where}: unsupported value ${v} (use a "string", an integer, or a YYYY-MM-DD date)`);
}

const ID_PATTERN = /^(RUSTSEC-\d{4}-\d{4}|GHSA(-[0-9a-z]{4}){3}|CVE-\d{4}-\d{4,})$/;
const AGE_KEYS = ["ecosystem", "package", "version", "reason", "expires"];
const VULN_KEYS = ["id", "package", "version", "reason", "expires"];

// Checks the parsed policy against ADR 0015 and returns
// { minAgeDays, ageExceptions, vulnExceptions, problems }, where `problems` are
// messages that each say what to change. `today` is a YYYY-MM-DD string (UTC).
export function validatePolicy(doc, today) {
  const problems = [];
  const known = new Set(["min_age_days", "age_exception", "vulnerability_exception"]);
  for (const k of Object.keys(doc)) {
    if (!known.has(k)) problems.push(`${POLICY}: unknown setting "${k}"`);
  }
  const minAgeDays = doc.min_age_days;
  if (!Number.isInteger(minAgeDays) || minAgeDays < 0) {
    problems.push(`${POLICY}: set min_age_days to a whole number of days`);
  }

  const latest = addDays(today, MAX_EXCEPTION_DAYS);
  const checkCommon = (e, label, keys) => {
    for (const k of Object.keys(e)) {
      if (!keys.includes(k)) problems.push(`${label}: unknown field "${k}" (fields are ${keys.join(", ")})`);
    }
    if (typeof e.reason !== "string" || !e.reason.trim()) {
      problems.push(`${label}: add a reason saying why this is acceptable and what ends it`);
    }
    const expires = e.expires?.date ?? (typeof e.expires === "string" ? e.expires : null);
    if (!expires || !isDate(expires)) {
      problems.push(`${label}: add expires = YYYY-MM-DD (no quotes), at most ${MAX_EXCEPTION_DAYS} days out`);
    } else if (expires < today) {
      problems.push(`${label}: expired on ${expires}; remove it and fix the dependency, or renew it with a new reason and expiry`);
    } else if (expires > latest) {
      problems.push(`${label}: expires ${expires} is more than ${MAX_EXCEPTION_DAYS} days out; use ${latest} or earlier`);
    }
    return expires;
  };

  const ageExceptions = (doc.age_exception ?? []).map((e, i) => {
    const label = `${POLICY}: age_exception #${i + 1} (${e.package ?? "?"}@${e.version ?? "?"})`;
    if (e.ecosystem !== "crates.io" && e.ecosystem !== "npm") {
      problems.push(`${label}: set ecosystem to "crates.io" or "npm"`);
    }
    for (const k of ["package", "version"]) {
      if (typeof e[k] !== "string" || !e[k]) problems.push(`${label}: set ${k} to the exact value in the lockfile`);
    }
    const expires = checkCommon(e, label, AGE_KEYS);
    return { ecosystem: e.ecosystem, package: e.package, version: e.version, reason: e.reason, expires };
  });

  const vulnExceptions = (doc.vulnerability_exception ?? []).map((e, i) => {
    const label = `${POLICY}: vulnerability_exception #${i + 1} (${e.id ?? "?"})`;
    if (typeof e.id !== "string" || !ID_PATTERN.test(e.id)) {
      problems.push(`${label}: set id to a RUSTSEC-, GHSA-, or CVE- advisory ID`);
    }
    for (const k of ["package", "version"]) {
      if (k in e && (typeof e[k] !== "string" || !e[k])) problems.push(`${label}: ${k}, when set, is the exact value in the lockfile`);
    }
    if ("version" in e && !("package" in e)) problems.push(`${label}: a version needs the package it belongs to`);
    const expires = checkCommon(e, label, VULN_KEYS);
    return { id: e.id, package: e.package ?? null, version: e.version ?? null, reason: e.reason, expires };
  });

  return { minAgeDays, ageExceptions, vulnExceptions, problems };
}

export function findAgeException(exceptions, ecosystem, name, version) {
  return exceptions.find((e) => e.ecosystem === ecosystem && e.package === name && e.version === version) ?? null;
}

// The age check's verdict on one package version published at `published`:
// "ok" when old enough, "allow" when younger but excepted, "fail" otherwise.
export function ageVerdict(policy, ecosystem, name, version, published, now) {
  const ageDays = (now - published) / DAY_MS;
  if (ageDays >= policy.minAgeDays) return { status: "ok", ageDays, exception: null };
  const exception = findAgeException(policy.ageExceptions, ecosystem, name, version);
  return { status: exception ? "allow" : "fail", ageDays, exception };
}

// The exception covering one OSV finding: its id is any of the finding's IDs
// or aliases, and its package and version, where set, are the finding's.
export function findVulnException(exceptions, finding) {
  const ids = new Set(finding.ids);
  return (
    exceptions.find(
      (e) =>
        ids.has(e.id) &&
        (e.package === null || e.package === finding.name) &&
        (e.version === null || e.version === finding.version),
    ) ?? null
  );
}

// One finding per OSV group (a group joins advisories that alias each other),
// as { ecosystem, name, version, ids, severity, lockfile }.
export function osvFindings(report) {
  const findings = [];
  for (const result of report.results ?? []) {
    for (const p of result.packages ?? []) {
      const groups = p.groups?.length
        ? p.groups.map((g) => ({ ids: [...(g.ids ?? []), ...(g.aliases ?? [])], severity: g.max_severity }))
        : (p.vulnerabilities ?? []).map((v) => ({ ids: [v.id, ...(v.aliases ?? [])] }));
      for (const g of groups) {
        findings.push({
          ecosystem: p.package.ecosystem,
          name: p.package.name,
          version: p.package.version,
          ids: [...new Set(g.ids)],
          severity: g.severity || null,
          lockfile: result.source?.path ?? "",
        });
      }
    }
  }
  return findings;
}

// A real calendar date in YYYY-MM-DD form: 2026-02-30 is not one.
export function isDate(s) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(s)) return false;
  const d = new Date(`${s}T00:00:00Z`);
  return !Number.isNaN(d.getTime()) && d.toISOString().slice(0, 10) === s;
}

export function addDays(date, days) {
  return new Date(Date.parse(`${date}T00:00:00Z`) + days * DAY_MS).toISOString().slice(0, 10);
}

// ---- Registries ----------------------------------------------------------------

class RegistryError extends CheckError {}

async function getJson(url) {
  let last;
  for (let attempt = 0; attempt < 3; attempt++) {
    if (attempt) await sleep(1000 * 2 ** attempt);
    try {
      const res = await fetch(url, { headers: { "User-Agent": USER_AGENT, Accept: "application/json" } });
      if (res.ok) return await res.json();
      last = `HTTP ${res.status}`;
      if (res.status === 404) break;
    } catch (err) {
      last = err.cause?.message ?? err.message;
    }
  }
  throw new RegistryError(`${url}: ${last}`);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function publishDate(url, value, field) {
  const date = new Date(value ?? NaN);
  if (Number.isNaN(date.getTime())) throw new RegistryError(`${url}: no readable ${field} in the response`);
  return date;
}

// crates.io asks crawlers for at most one request per second.
let lastCratesIo = 0;
async function cratesIoPublished(name, version) {
  await sleep(Math.max(0, lastCratesIo + 1000 - Date.now()));
  lastCratesIo = Date.now();
  const url = `https://crates.io/api/v1/crates/${encodeURIComponent(name)}/${encodeURIComponent(version)}`;
  const body = await getJson(url);
  return publishDate(url, body.version?.created_at, "version.created_at");
}

const npmDocs = new Map();
async function npmPublished(name, version) {
  const url = `https://registry.npmjs.org/${name.replace("/", "%2F")}`;
  if (!npmDocs.has(name)) npmDocs.set(name, getJson(url));
  const body = await npmDocs.get(name);
  return publishDate(url, body.time?.[version], `time["${version}"]`);
}

// ---- Repository ------------------------------------------------------------------

function run(program, args) {
  const result = spawnSync(program, args, { cwd: ROOT, encoding: "utf8", maxBuffer: MAX_BUFFER });
  if (result.error || result.status !== 0) {
    throw new CheckError(`${program} ${args.join(" ")} failed: ${result.error?.message ?? result.stderr.trim()}`);
  }
  return result.stdout;
}

function trackedLockfiles() {
  const lockfiles = lockfilesFrom(run("git", ["ls-files", "--", "*Cargo.lock", "*pnpm-lock.yaml"]));
  if (!lockfiles.length) throw new CheckError("git tracks no Cargo.lock or pnpm-lock.yaml");
  return lockfiles;
}

function baseFile(ref, path) {
  const exists = spawnSync("git", ["cat-file", "-e", `${ref}:${path}`], { cwd: ROOT });
  return exists.status === 0 ? run("git", ["show", `${ref}:${path}`]) : "";
}

// The members of the Cargo workspace whose lockfile is `lockPath`, as name@version.
const membersByLockfile = new Map();
function workspaceMembers(lockPath) {
  if (!membersByLockfile.has(lockPath)) {
    const manifest = join(ROOT, dirname(lockPath), "Cargo.toml");
    const metadata = JSON.parse(run("cargo", ["metadata", "--no-deps", "--format-version", "1", "--manifest-path", manifest]));
    membersByLockfile.set(lockPath, new Set(metadata.packages.map(key)));
  }
  return membersByLockfile.get(lockPath);
}

// The members of every Cargo workspace whose lockfile git tracks, as
// name@version: the repository's own crates (Core's and the Shell's, all in
// the root workspace). They come from this repository, not a registry, so
// they have no publish time to judge.
function repositoryCrates() {
  const all = new Set();
  for (const lock of trackedLockfiles()) {
    if (lock.ecosystem === "crates.io") for (const member of workspaceMembers(lock.path)) all.add(member);
  }
  return all;
}

// ---- Modes ---------------------------------------------------------------------

async function ageMode(policy, base, now) {
  if (spawnSync("git", ["rev-parse", "--verify", "--quiet", `${base}^{commit}`], { cwd: ROOT }).status !== 0) {
    throw new CheckError(`base ${base} is not a commit here. Fetch it (git fetch origin) or pass --base <ref>.`);
  }
  const failures = [];
  const errors = [];
  let checked = 0;
  for (const lock of trackedLockfiles()) {
    const before = parseLockfile(lock.ecosystem, baseFile(base, lock.path), `${lock.path} at ${base}`);
    const after = parseLockfile(lock.ecosystem, readFileSync(join(ROOT, lock.path), "utf8"), lock.path);
    for (const p of diffPackages(before, after)) {
      const id = `${lock.ecosystem} ${p.name}@${p.version}`;
      const members = lock.ecosystem === "crates.io" && p.source === null ? repositoryCrates() : new Set();
      const kind = classifyPackage(lock.ecosystem, p, members);
      if (kind === "workspace") continue;
      checked++;
      if (kind === "unregistered") {
        const from = p.source ?? "a path outside the repository's workspaces";
        const exception = findAgeException(policy.ageExceptions, lock.ecosystem, p.name, p.version);
        if (exception) console.log(`  allow ${id}: not from the registry (${from}); exception until ${exception.expires}: ${exception.reason}`);
        else failures.push(`${id}: not from the registry (${from}), so its age cannot be checked`);
        continue;
      }
      let published;
      try {
        published = lock.ecosystem === "npm" ? await npmPublished(p.name, p.version) : await cratesIoPublished(p.name, p.version);
      } catch (err) {
        if (!(err instanceof RegistryError)) throw err;
        errors.push(`${id}: ${err.message}`);
        continue;
      }
      const { status, ageDays, exception } = ageVerdict(policy, lock.ecosystem, p.name, p.version, published, now);
      const shown = `${ageDays.toFixed(1)} days old (published ${published.toISOString()})`;
      if (status === "ok") console.log(`  ok    ${id}: ${shown}`);
      else if (status === "allow") console.log(`  allow ${id}: ${shown}; exception until ${exception.expires}: ${exception.reason}`);
      else failures.push(`${id}: ${shown}, younger than ${policy.minAgeDays} days`);
    }
  }
  console.log(`Checked ${checked} package version(s) added or changed since ${base}.`);
  if (errors.length) {
    console.error(
      `\nRegistry error: could not read the publish time of ${errors.length} package version(s). This is not a policy failure: re-run the job, and if it persists check the version exists on the registry.`,
    );
    for (const e of errors) console.error(`  ${e}`);
  }
  if (failures.length) {
    console.error(`\nPolicy failure: ${failures.length} package version(s) are too young or have no registry publish time.`);
    for (const f of failures) console.error(`  ${f}`);
    console.error(
      `\nWait until they are ${policy.minAgeDays} days old, use a registry release, pin an older version, or add an [[age_exception]] to ${POLICY} with a reason and an expiry (docs/dependencies.md).`,
    );
    return 1;
  }
  return errors.length ? 2 : 0;
}

function vulnsMode(policy) {
  const lockfiles = trackedLockfiles().map((l) => l.path);
  const scanner = process.env.OSV_SCANNER || "osv-scanner";
  const args = ["scan", "source", "--format", "json", ...lockfiles.flatMap((p) => ["--lockfile", p])];
  const scan = spawnSync(scanner, args, { cwd: ROOT, encoding: "utf8", maxBuffer: MAX_BUFFER });
  // OSV-Scanner exits 0 when clean and 1 when it found vulnerabilities; anything else is an error.
  if (scan.error || (scan.status !== 0 && scan.status !== 1)) {
    throw new CheckError(`${scanner} ${args.join(" ")} failed.\n${scan.error?.message ?? scan.stderr}`);
  }
  let report;
  try {
    report = JSON.parse(scan.stdout);
  } catch {
    throw new CheckError(`${scanner} did not print JSON.\n${scan.stderr}`);
  }

  const failures = [];
  const used = new Set();
  for (const f of osvFindings(report)) {
    const id = `${f.ecosystem} ${f.name}@${f.version}: ${f.ids.join(" / ")}${f.severity ? ` (CVSS ${f.severity})` : ""}`;
    const exception = findVulnException(policy.vulnExceptions, f);
    if (exception) {
      used.add(exception);
      console.log(`  allow ${id}; exception until ${exception.expires}: ${exception.reason}`);
    } else {
      failures.push(`${id} in ${f.lockfile}`);
    }
  }
  console.log(`Scanned ${lockfiles.join(", ")}.`);
  for (const e of policy.vulnExceptions) {
    if (!used.has(e)) console.log(`  note  vulnerability_exception ${e.id} matched nothing; remove it from ${POLICY}.`);
  }
  if (failures.length) {
    console.error(`\nPolicy failure: ${failures.length} known vulnerabilit${failures.length === 1 ? "y" : "ies"}.`);
    for (const f of failures) console.error(`  ${f}`);
    console.error(
      `\nUpdate the package to a fixed version (an age_exception lets a fresh fix in), or add a [[vulnerability_exception]] to ${POLICY} with a reason and an expiry (docs/dependencies.md).`,
    );
    return 1;
  }
  console.log("No known vulnerabilities without an exception.");
  return 0;
}

async function main(argv) {
  const mode = argv[0];
  const baseAt = argv.indexOf("--base");
  const base = baseAt > 0 ? argv[baseAt + 1] : "origin/main";
  if ((mode !== "age" && mode !== "vulns") || (baseAt > 0 && !base)) {
    console.error("usage: node scripts/deps-check.mjs age [--base <ref>] | vulns");
    return 2;
  }
  const now = new Date();
  const today = now.toISOString().slice(0, 10);
  const text = readFileSync(join(ROOT, POLICY), "utf8");
  let policy;
  try {
    policy = validatePolicy(parsePolicyToml(text), today);
  } catch (err) {
    console.error(`Policy failure: ${err.message}`);
    return 1;
  }
  if (policy.problems.length) {
    console.error(`Policy failure: ${POLICY} needs fixing.`);
    for (const p of policy.problems) console.error(`  ${p}`);
    return 1;
  }
  return mode === "age" ? ageMode(policy, base, now) : vulnsMode(policy);
}

// Anything that stops the check from finishing exits 2, so it is never
// mistaken for a policy failure (1).
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    process.exitCode = await main(process.argv.slice(2));
  } catch (err) {
    console.error("Could not run the check; this is not a policy failure.");
    console.error(err instanceof CheckError ? err.message : err);
    process.exitCode = 2;
  }
}
