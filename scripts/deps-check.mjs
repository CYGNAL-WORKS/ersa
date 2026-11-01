#!/usr/bin/env node
// Enforces the dependency policy of ADR 0015 against the lockfiles.
//
//   node scripts/deps-check.mjs age [--base <ref>]   release age of what changed since <ref>
//   node scripts/deps-check.mjs vulns                 known vulnerabilities in every lockfile
//
// `age` compares the lockfiles on disk with those at <ref> (default origin/main)
// and looks up the publish time of every package version added or changed.
// `vulns` runs OSV-Scanner (on PATH, or the path in OSV_SCANNER).
// Both read deps-policy.toml for the threshold and the exceptions.
//
// Exit status: 0 pass, 1 policy failure, 2 the check could not run
// (registry or scanner error). docs/dependencies.md explains what to do.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const POLICY = "deps-policy.toml";
const LOCKFILES = [
  { path: "Cargo.lock", ecosystem: "crates.io", parse: (t) => parseCargoLock(t) },
  { path: "pnpm-lock.yaml", ecosystem: "npm", parse: (t) => parsePnpmLock(t) },
];
const CRATES_IO_SOURCES = new Set([
  "registry+https://github.com/rust-lang/crates.io-index",
  "sparse+https://index.crates.io/",
]);
const USER_AGENT = "ersa-deps-check (https://github.com/KPandaK/ersa)";
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

// Every registry package in a pnpm-lock.yaml (v6 and later) as
// { name, version, source }. Keys come from the top-level `packages:` map,
// such as `lodash@4.17.21`, `'@types/node@22.1.0'`, or (v6)
// `/react-dom@18.2.0(react@18.2.0)`. Versions that are not plain semver
// (`file:`, `link:`, tarball and git URLs) are not from the registry.
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
    const at = key.lastIndexOf("@");
    if (at <= 0) continue;
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
const VULN_KEYS = ["id", "reason", "expires"];

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
    const expires = checkCommon(e, label, VULN_KEYS);
    return { id: e.id, reason: e.reason, expires };
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

// The exception covering one OSV finding, matched on any of its IDs or aliases.
export function findVulnException(exceptions, ids) {
  const all = new Set(ids);
  return exceptions.find((e) => all.has(e.id)) ?? null;
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

function isDate(s) {
  return /^\d{4}-\d{2}-\d{2}$/.test(s) && !Number.isNaN(Date.parse(`${s}T00:00:00Z`));
}

export function addDays(date, days) {
  return new Date(Date.parse(`${date}T00:00:00Z`) + days * DAY_MS).toISOString().slice(0, 10);
}

// ---- Registries ----------------------------------------------------------------

class RegistryError extends Error {}

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

// crates.io asks crawlers for at most one request per second.
let lastCratesIo = 0;
async function cratesIoPublished(name, version) {
  await sleep(Math.max(0, lastCratesIo + 1000 - Date.now()));
  lastCratesIo = Date.now();
  const url = `https://crates.io/api/v1/crates/${encodeURIComponent(name)}/${encodeURIComponent(version)}`;
  const body = await getJson(url);
  if (!body.version?.created_at) throw new RegistryError(`${url}: no version.created_at in the response`);
  return new Date(body.version.created_at);
}

const npmDocs = new Map();
async function npmPublished(name, version) {
  const url = `https://registry.npmjs.org/${name.replace("/", "%2F")}`;
  if (!npmDocs.has(name)) npmDocs.set(name, getJson(url));
  const time = (await npmDocs.get(name)).time?.[version];
  if (!time) throw new RegistryError(`${url}: no time["${version}"] in the response`);
  return new Date(time);
}

// ---- Modes ---------------------------------------------------------------------

function git(...args) {
  return execFileSync("git", args, { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function baseFile(ref, path) {
  try {
    git("cat-file", "-e", `${ref}:${path}`);
  } catch {
    return "";
  }
  return git("show", `${ref}:${path}`);
}

async function ageMode(policy, base, now) {
  try {
    git("rev-parse", "--verify", "--quiet", `${base}^{commit}`);
  } catch {
    console.error(`Base ${base} is not a commit here. Fetch it (git fetch origin) or pass --base <ref>.`);
    return 2;
  }
  const failures = [];
  const errors = [];
  let checked = 0;
  for (const lock of LOCKFILES) {
    const file = join(ROOT, lock.path);
    if (!existsSync(file)) continue;
    const changed = diffPackages(lock.parse(baseFile(base, lock.path)), lock.parse(readFileSync(file, "utf8")));
    for (const p of changed) {
      const id = `${lock.ecosystem} ${p.name}@${p.version}`;
      if (p.source === null) continue; // a workspace crate
      const fromRegistry = lock.ecosystem === "npm" ? p.source === "npm" : CRATES_IO_SOURCES.has(p.source);
      if (!fromRegistry) {
        console.log(`  skip  ${id}: not from the registry (${p.source}); cargo-deny judges sources`);
        continue;
      }
      checked++;
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
    console.error(`\nRegistry error: could not read the publish time of ${errors.length} package version(s). This is not a policy failure: re-run the job, and if it persists check the version exists on the registry.`);
    for (const e of errors) console.error(`  ${e}`);
  }
  if (failures.length) {
    console.error(`\nPolicy failure: ${failures.length} package version(s) are younger than ${policy.minAgeDays} days.`);
    for (const f of failures) console.error(`  ${f}`);
    console.error(
      `\nWait until they are ${policy.minAgeDays} days old, pin an older version, or add an [[age_exception]] to ${POLICY} with a reason and an expiry (docs/dependencies.md).`,
    );
    return 1;
  }
  return errors.length ? 2 : 0;
}

function vulnsMode(policy) {
  const lockfiles = LOCKFILES.map((l) => l.path).filter((p) => existsSync(join(ROOT, p)));
  const scanner = process.env.OSV_SCANNER || "osv-scanner";
  const args = ["scan", "source", "--format", "json", ...lockfiles.flatMap((p) => ["--lockfile", p])];
  const run = spawnSync(scanner, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  // OSV-Scanner exits 0 when clean and 1 when it found vulnerabilities; anything else is an error.
  if (run.error || (run.status !== 0 && run.status !== 1)) {
    console.error(`Scanner error: ${scanner} ${args.join(" ")} failed. This is not a policy failure.`);
    console.error(run.error?.message ?? run.stderr);
    return 2;
  }
  let report;
  try {
    report = JSON.parse(run.stdout);
  } catch {
    console.error(`Scanner error: ${scanner} did not print JSON. This is not a policy failure.\n${run.stderr}`);
    return 2;
  }

  const failures = [];
  const used = new Set();
  for (const f of osvFindings(report)) {
    const id = `${f.ecosystem} ${f.name}@${f.version}: ${f.ids.join(" / ")}${f.severity ? ` (CVSS ${f.severity})` : ""}`;
    const exception = findVulnException(policy.vulnExceptions, f.ids);
    if (exception) {
      used.add(exception);
      console.log(`  allow ${id}; exception until ${exception.expires}: ${exception.reason}`);
    } else {
      failures.push(id);
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
  let policy;
  try {
    policy = validatePolicy(parsePolicyToml(readFileSync(join(ROOT, POLICY), "utf8")), today);
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

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await main(process.argv.slice(2));
}
