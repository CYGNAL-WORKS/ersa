#!/usr/bin/env node
// The harness pass's first step (docs/agents/orchestration.md, Train the
// harness): for each pull request merged since the last pass, what Docket's
// merge review found, and which reviewer found it.
//
//   node scripts/harness-lessons.mjs [--since <commit>]
//
// The last pass is the newest commit on origin/main whose subject starts
// "Harness: lessons from"; --since names another start. The pull
// requests are the first-parent merges since it ("Merge pull request #n"),
// the pass's own left out. For each, the merge reviews this account posted
// (bodies starting <!-- merge-review -->, each carrying its head, verdict and
// findings as JSON, every finding naming the reviewers that reported it;
// another account's are ignored, since anyone can post one) are read, and
// every blocking finding (high or medium) with origin "new" is listed and
// counted against each reviewer that found it. A reviewer whose count stays at
// zero changed no merge decision in the batch. A pull request with no merge
// review is listed as not counted. A review marked superseded is not a round,
// but its findings count: the review after it carries them as listed.
//
// GitHub is read once per pull request and the answer kept under
// .scratch/harness-lessons/<n>.json, so a second run, or one after the API
// quota is spent, costs nothing; a merged pull request's reviews do not change.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const PASS_SUBJECT = /^Harness: lessons from/;

// The record a merge review body carries: the marker line, then the marker with the JSON.
export function parseState(body) {
  const m = /^<!-- merge-review -->\s*<!-- merge-review (.*?) -->/s.exec(body ?? "");
  try {
    return m ? JSON.parse(m[1]) : null;
  } catch {
    return null;
  }
}

// A stalled git or gh call ends after two minutes, as in the review and the gate.
function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: 2 * 60 * 1000 });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: [(r.stderr ?? "").trim(), r.error?.message].filter(Boolean).join("; ") };
}

// The pull request numbers merged by first-parent merge commits in `log`
// (one "<sha> <subject>" per line). The pass's own merge is among them, since
// a merge commit's subject names only its pull request; main() leaves it out
// by the pull request's title.
export function mergedPullRequests(log) {
  return log
    .split("\n")
    .map((l) => /Merge pull request #(\d+)/.exec(l)?.[1])
    .filter(Boolean)
    .map(Number);
}

// One pull request's blocking findings at merge, each with the reviewers that
// found it. Only reviews by `author` (the account the merge review posts as)
// count: anyone can post a review with the marker on a public repository. A
// superseded review's findings count (the next review carries them as
// listed), the review itself is not a round.
export function compare(pr, author) {
  // Only a state of the shape the review writes: a body this account posted by
  // hand, or a truncated one, is not a review.
  const states = (pr.reviews ?? [])
    .filter((r) => r.author?.login === author)
    .map((r) => parseState(r.body))
    .filter((s) => typeof s?.head === "string" && Array.isArray(s.findings));
  if (!states.length) return { number: pr.number, title: pr.title, compared: false, why: "no merge review" };
  const seen = new Set();
  const findings = [];
  for (const s of states) {
    for (const f of s.findings) {
      if (!f || f.severity === "low" || f.origin !== "new") continue;
      const key = `${f.location}|${f.defect}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const reviewers = Array.isArray(f.reviewers) ? f.reviewers.filter((x) => typeof x === "string") : [];
      findings.push({ head: s.head.slice(0, 7), severity: f.severity, location: f.location, defect: f.defect, reviewers });
    }
  }
  const rounds = states.filter((s) => !s.superseded);
  return { number: pr.number, title: pr.title, compared: true, rounds: rounds.length, blocked: rounds.filter((s) => s.verdict === "blocked").length, findings };
}

// Blocking findings per reviewer, most first; a finding two reviewers both
// reported counts for each. "unnamed" holds findings whose record names none.
export function tally(results) {
  const counts = new Map();
  for (const f of results.filter((r) => r.compared).flatMap((r) => r.findings)) {
    for (const name of f.reviewers.length ? f.reviewers : ["unnamed"]) counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}

export function report(results, author) {
  const lines = [];
  const compared = results.filter((r) => r.compared);
  const findings = compared.flatMap((r) => r.findings);
  // The login is named so a run under another account (a second gh login, a
  // CI token) that finds no merge review anywhere says why.
  lines.push(`${results.length} pull request(s) merged since the last pass; ${compared.length} counted, from merge reviews posted by ${author}; ${findings.length} blocking finding(s) at merge.`);
  const counts = tally(results);
  lines.push(`Blocking findings by reviewer: ${counts.length ? counts.map(([name, n]) => `${name} ${n}`).join(", ") : "none"}. A reviewer in .claude/review.json that is not listed found none.`);
  for (const r of results) {
    if (!r.compared) {
      lines.push(`\n#${r.number} ${r.title}: not counted (${r.why}).`);
      continue;
    }
    lines.push(`\n#${r.number} ${r.title}: ${r.rounds} review(s), ${r.blocked} blocked.`);
    for (const f of r.findings) lines.push(`  - ${f.severity}, ${f.location} (${f.head}), found by ${f.reviewers.join(", ") || "unnamed"}: ${f.defect}`);
  }
  return lines.join("\n");
}

// Reads what one pull request needs, from the cache or GitHub. A cached file
// that does not parse (a write cut short) is read again from GitHub; the
// write goes through a rename, so a cut leaves no half file behind.
function fetchPullRequest(n, { run: r, cacheDir }) {
  const file = join(cacheDir, `${n}.json`);
  if (existsSync(file)) {
    try {
      const cached = JSON.parse(readFileSync(file, "utf8"));
      if (cached && typeof cached === "object" && !Array.isArray(cached)) return cached;
    } catch {}
  }
  const x = r("gh", ["pr", "view", String(n), "--json", "number,title,body,reviews"]);
  // The whole error on one line: a timeout is appended after gh's own lines.
  if (!x.ok) throw new Error(`gh pr view ${n} failed: ${x.err.replace(/\s*\n\s*/g, " ").slice(0, 300)}`);
  let pr;
  try {
    pr = JSON.parse(x.out);
  } catch {
    throw new Error(`gh pr view ${n} answered no JSON`);
  }
  mkdirSync(cacheDir, { recursive: true });
  writeFileSync(`${file}.tmp`, JSON.stringify(pr));
  renameSync(`${file}.tmp`, file);
  return pr;
}

export function main(argv, { run: r = run, cacheDir = join(ROOT, ".scratch", "harness-lessons") } = {}) {
  try {
    return lessons(argv, r, cacheDir);
  } catch (e) {
    return { code: 2, stderr: `${e.message}\n` };
  }
}

function lessons(argv, r, cacheDir) {
  const at = argv.indexOf("--since");
  let since = at >= 0 ? argv[at + 1] : "";
  if (!since) {
    // The pass's commit sits on its own branch, so it is an ancestor of main
    // and not on main's first-parent line.
    const passes = r("git", ["log", "--format=%H %s", "origin/main"]);
    if (!passes.ok) return { code: 2, stderr: `git log failed: ${passes.err}\n` };
    since = passes.out.split("\n").find((l) => PASS_SUBJECT.test(l.replace(/^\S+\s+/, "")))?.split(" ")[0];
    if (!since) return { code: 2, stderr: "no commit on origin/main has a subject starting \"Harness: lessons from\"; pass --since <commit>\n" };
  }
  const log = r("git", ["log", "--first-parent", "--merges", "--format=%h %s", `${since}..origin/main`]);
  if (!log.ok) return { code: 2, stderr: `git log failed: ${log.err}\n` };
  const me = r("gh", ["api", "user", "--jq", ".login"]);
  if (!me.ok || !me.out) return { code: 2, stderr: `gh api user failed: ${me.err || "no login"}\n` };
  const results = mergedPullRequests(log.out)
    .map((n) => fetchPullRequest(n, { run: r, cacheDir }))
    .filter((pr) => !PASS_SUBJECT.test(pr.title ?? ""))
    .map((pr) => compare(pr, me.out));
  return { code: 0, stdout: `${report(results, me.out)}\n` };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const { code, stdout, stderr } = main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
