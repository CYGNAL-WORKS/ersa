#!/usr/bin/env node
// Reads the open pull requests and puts each in one state, with no model involved,
// so the PR shepherd (.claude/skills/shepherd-prs) wakes only when something changed.
//
//   node scripts/pr-queue.mjs            print the queue
//   node scripts/pr-queue.mjs --changed  print it and exit 0 only if it differs from
//                                        the last --changed run; exit 1 otherwise
//
// States, first match wins: draft, conflicting, ci-running, ci-red (with the
// failure's signature), behind-main, green. A signature is the failing job's
// first error line or its failed test names; one shared by two or more pull
// requests is marked shared, since one cause (a runner, a flaky test) then
// blocks them all. Failed logs are fetched once per head commit and cached.
//
// The last run lives in .scratch/pr-queue/last.json (uncommitted). The
// fingerprint covers number, head, state and signature, never timestamps, so
// an unchanged queue stays unchanged.

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const STATE_DIR = join(ROOT, ".scratch", "pr-queue");
const LAST = join(STATE_DIR, "last.json");

export const NEVER_STARTED = "job ran no steps (no runner took it)";

// The cause of a failed job, from its `gh run view --log-failed` output: the
// failed test names if any, else the first error line that is not the generic
// exit-code line, or the first line above that one that names an error.
export function signature(log) {
  const lines = log.split(/\r?\n/).map((l) => l.replace(/^.*?\d{4}-\d\d-\d\dT[\d:.]+Z\s?/, "").trim());
  const tests = [...new Set(lines.map((l) => /^test (\S+) \.\.\. FAILED$/.exec(l)?.[1]).filter(Boolean))].sort();
  if (tests.length) return `tests failed: ${tests.join(", ")}`;
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].startsWith("##[error]")) continue;
    const message = lines[i].slice("##[error]".length).trim();
    if (!/^Process completed with exit code \d+\.?$/.test(message)) return message;
    let start = i;
    while (start > 0 && !lines[start - 1].startsWith("##[")) start--;
    const output = lines.slice(start, i).filter(Boolean);
    return output.find((l) => /\berror\b|not found|denied|fatal|failed/i.test(l)) ?? output.at(-1) ?? message;
  }
  return "failed, no error line in the log";
}

// One state per pull request. `failures` maps a failed check's name to its
// signature; `containsMain` says whether the head contains main's tip.
export function classify(pr, { failures, containsMain }) {
  const checks = pr.statusCheckRollup ?? [];
  if (pr.isDraft) return { state: "draft" };
  if (pr.mergeable === "CONFLICTING") return { state: "conflicting" };
  if (checks.some((c) => c.status && c.status !== "COMPLETED")) return { state: "ci-running" };
  const failed = checks.filter((c) => ["FAILURE", "TIMED_OUT", "CANCELLED", "STARTUP_FAILURE"].includes(c.conclusion));
  if (failed.length) {
    const signatures = [...new Set(failed.map((c) => failures[c.name] ?? "failed"))].sort();
    return { state: "ci-red", signature: signatures.join(" | ") };
  }
  if (!containsMain) return { state: "behind-main" };
  return { state: "green" };
}

// Marks rows whose signature another row shares.
export function markShared(rows) {
  const count = new Map();
  for (const r of rows) if (r.signature) count.set(r.signature, (count.get(r.signature) ?? 0) + 1);
  return rows.map((r) => (r.signature && count.get(r.signature) > 1 ? { ...r, shared: true } : r));
}

export function fingerprint(rows) {
  const key = rows
    .map((r) => [r.number, r.head, r.state, r.signature ?? ""])
    .sort((a, b) => a[0] - b[0]);
  return createHash("sha256").update(JSON.stringify(key)).digest("hex").slice(0, 16);
}

// Rows that are new, gone, or in another state or signature than last time.
export function changes(previous, rows) {
  const before = new Map((previous ?? []).map((r) => [r.number, r]));
  const now = new Map(rows.map((r) => [r.number, r]));
  const out = [];
  for (const r of rows) {
    const p = before.get(r.number);
    if (!p) out.push({ number: r.number, change: "new", to: r.state });
    else if (p.head !== r.head || p.state !== r.state || (p.signature ?? "") !== (r.signature ?? ""))
      out.push({ number: r.number, change: p.head !== r.head ? "new commits" : "state", from: p.state, to: r.state });
  }
  for (const [n, p] of before) if (!now.has(n)) out.push({ number: n, change: "closed or merged", from: p.state });
  return out.sort((a, b) => a.number - b.number);
}

function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return { ok: r.status === 0, out: r.stdout ?? "", err: r.stderr ?? "" };
}

function main() {
  const changedMode = process.argv.includes("--changed");
  const last = existsSync(LAST) ? JSON.parse(readFileSync(LAST, "utf8")) : { rows: [], logs: {} };

  run("git", ["fetch", "-q", "origin"]);
  const list = run("gh", [
    "pr", "list", "--state", "open", "--limit", "100",
    "--json", "number,title,headRefName,headRefOid,isDraft,mergeable,statusCheckRollup",
  ]);
  if (!list.ok) {
    process.stderr.write(`gh pr list failed: ${list.err}`);
    process.exit(2);
  }

  const logs = {};
  const jobs = {};
  const stepCount = (runId, name) => {
    jobs[runId] ??= JSON.parse(run("gh", ["run", "view", runId, "--json", "jobs"]).out || '{"jobs":[]}').jobs;
    return jobs[runId].find((j) => j.name === name)?.steps?.length;
  };
  const rows = JSON.parse(list.out).map((pr) => {
    const failures = {};
    for (const c of pr.statusCheckRollup ?? []) {
      if (!["FAILURE", "TIMED_OUT", "STARTUP_FAILURE"].includes(c.conclusion)) continue;
      const key = `${pr.headRefOid}:${c.name}`;
      if (!(key in last.logs)) {
        const id = /\/runs\/(\d+)/.exec(c.detailsUrl ?? "")?.[1];
        const log = id ? run("gh", ["run", "view", id, "--log-failed"]) : { out: "" };
        // The log covers every failed job of the run; keep this job's lines.
        const own = log.out.split(/\r?\n/).filter((l) => l.startsWith(c.name + "\t")).join("\n");
        last.logs[key] = !own && id && stepCount(id, c.name) === 0 ? NEVER_STARTED : signature(own || log.out);
      }
      logs[key] = failures[c.name] = last.logs[key];
    }
    const containsMain = run("git", ["merge-base", "--is-ancestor", "origin/main", pr.headRefOid]).ok;
    return { number: pr.number, title: pr.title, head: pr.headRefOid.slice(0, 7), ...classify(pr, { failures, containsMain }) };
  });
  const marked = markShared(rows);
  const print = fingerprint(marked);
  const diff = changes(last.rows, marked);

  const changed = print !== last.fingerprint;
  // An unchanged run prints nothing in --changed mode: whatever waits on it
  // reads only the run that changed.
  if (!changedMode || changed) {
    for (const r of marked) {
      const cause = r.signature ? `  ${r.shared ? "[shared] " : ""}${r.signature.slice(0, 160)}` : "";
      process.stdout.write(`#${r.number}\t${r.state}\t${r.title.slice(0, 60)}${cause}\n`);
    }
    if (diff.length) {
      process.stdout.write("\nChanged since the last run:\n");
      for (const d of diff) process.stdout.write(`#${d.number}\t${d.change}\t${d.from ?? ""} -> ${d.to ?? ""}\n`);
    }
  }

  if (!changedMode) process.exit(0);
  mkdirSync(STATE_DIR, { recursive: true });
  writeFileSync(LAST, JSON.stringify({ fingerprint: print, rows: marked, logs }, null, 2));
  process.exit(changed ? 0 : 1);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
