#!/usr/bin/env node
// The harness pass's first step (docs/agents/orchestration.md, Train the
// harness): for each pull request merged since the last pass, what the merge
// review found against what the author's own review listed.
//
//   node scripts/harness-lessons.mjs [--since <commit>] [--json]
//
// The last pass is the newest commit on origin/main whose subject starts
// "Harness: lessons from"; --since names another start. The pull
// requests are the first-parent merges since it ("Merge pull request #n"),
// the pass's own left out. For each, the merge reviews this repository posted
// (bodies starting <!-- merge-review -->, each carrying its head, verdict and
// findings as JSON) are read, and every blocking finding (high or medium) with
// origin "new" is set against the body's **Review** section: the finding's
// file is looked for there, so "not named" is a lead, not a verdict. A pull
// request with no merge review or no Review section is listed as not
// compared. Findings marked superseded are left out: that head was never a
// round.
//
// GitHub is read once per pull request and the answer kept under
// .scratch/harness-lessons/<n>.json, so a second run, or one after the API
// quota is spent, costs nothing. --json prints the comparison as one object
// for another tool to read.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
export const PASS_SUBJECT = /^Harness: lessons from/;
const MARK = "<!-- merge-review -->";

function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() };
}

// The JSON a merge review's body carries, or null for any other review.
export function parseState(body) {
  const m = /^<!-- merge-review -->\s*<!-- merge-review (.*?) -->/s.exec(body ?? "");
  try {
    return m ? JSON.parse(m[1]) : null;
  } catch {
    return null;
  }
}

// The body's "## Review" section: from that heading to the next "## ".
export function reviewSection(body) {
  const m = /^##\s+Review\s*$([\s\S]*?)(?=^##\s|(?![\s\S]))/m.exec(body ?? "");
  return m ? m[1].trim() : "";
}

// The pull request numbers merged by first-parent merge commits in `log`
// (one "<sha> <subject>" per line), the pass's own left out.
export function mergedPullRequests(log) {
  return log
    .split("\n")
    .filter((l) => l.trim() && !PASS_SUBJECT.test(l.replace(/^\S+\s+/, "")))
    .map((l) => /Merge pull request #(\d+)/.exec(l)?.[1])
    .filter(Boolean)
    .map(Number);
}

const fileOf = (location) => (location ?? "").split(":")[0].split("/").pop().split(" ")[0];

// One pull request's comparison: the blocking findings the merge review posted,
// each with whether the author's Review section names its file.
export function compare(pr) {
  const states = (pr.reviews ?? []).map((r) => parseState(r.body)).filter(Boolean);
  const review = reviewSection(pr.body);
  if (!states.length) return { number: pr.number, title: pr.title, compared: false, why: "no merge review" };
  if (!review) return { number: pr.number, title: pr.title, compared: false, why: "no Review section in the body" };
  const seen = new Set();
  const findings = [];
  for (const s of states) {
    if (s.superseded) continue;
    for (const f of s.findings ?? []) {
      if (f.severity === "low" || f.origin !== "new") continue;
      const key = `${f.location}|${f.defect}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const file = fileOf(f.location);
      findings.push({ head: s.head.slice(0, 7), severity: f.severity, location: f.location, defect: f.defect, named: Boolean(file) && review.includes(file) });
    }
  }
  return { number: pr.number, title: pr.title, compared: true, rounds: states.filter((s) => !s.superseded).length, blocked: states.filter((s) => s.verdict === "blocked" && !s.superseded).length, findings };
}

export function report(results) {
  const lines = [];
  const compared = results.filter((r) => r.compared);
  const findings = compared.flatMap((r) => r.findings.map((f) => ({ ...f, pr: r.number })));
  const unnamed = findings.filter((f) => !f.named);
  lines.push(`${results.length} pull request(s) merged since the last pass; ${compared.length} compared; ${findings.length} blocking finding(s) at merge, ${unnamed.length} not named in the author's Review section.`);
  for (const r of results) {
    if (!r.compared) {
      lines.push(`\n#${r.number} ${r.title}: not compared (${r.why}).`);
      continue;
    }
    lines.push(`\n#${r.number} ${r.title}: ${r.rounds} review(s), ${r.blocked} blocked.`);
    for (const f of r.findings) lines.push(`  - ${f.severity}, ${f.location} (${f.head}), ${f.named ? "named" : "NOT NAMED"} by the author: ${f.defect}`);
  }
  return lines.join("\n");
}

// Reads what one pull request needs, from the cache or GitHub.
function fetchPullRequest(n, { run: r, cacheDir }) {
  const file = join(cacheDir, `${n}.json`);
  if (existsSync(file)) return JSON.parse(readFileSync(file, "utf8"));
  const x = r("gh", ["pr", "view", String(n), "--json", "number,title,body,reviews"]);
  if (!x.ok) throw new Error(`gh pr view ${n} failed: ${x.err.split("\n")[0]}`);
  const pr = JSON.parse(x.out);
  mkdirSync(cacheDir, { recursive: true });
  writeFileSync(file, JSON.stringify(pr));
  return pr;
}

export function main(argv, { run: r = run, cacheDir = join(ROOT, ".scratch", "harness-lessons") } = {}) {
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
  const results = mergedPullRequests(log.out).map((n) => compare(fetchPullRequest(n, { run: r, cacheDir })));
  return { code: 0, stdout: `${argv.includes("--json") ? JSON.stringify(results, null, 2) : report(results)}\n` };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const { code, stdout, stderr } = main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
