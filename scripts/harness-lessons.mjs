#!/usr/bin/env node
// The harness pass's first step (docs/agents/orchestration.md, Train the
// harness): for each pull request merged since the last pass, what the merge
// review found against what the author's own review listed.
//
//   node scripts/harness-lessons.mjs [--since <commit>]
//
// The last pass is the newest commit on origin/main whose subject starts
// "Harness: lessons from"; --since names another start. The pull
// requests are the first-parent merges since it ("Merge pull request #n"),
// the pass's own left out. For each, the merge reviews this account posted
// (bodies starting <!-- merge-review -->, each carrying its head, verdict and
// findings as JSON; another account's are ignored, since anyone can post one)
// are read, and every blocking finding (high or medium) with origin "new" is
// set against the body's **Review** section: the finding's file is looked for
// there, so "not named" is a lead, not a verdict. A pull request with no merge
// review or no Review section is listed as not compared. A review marked
// superseded is not a round, but its findings count: the review after it
// carries them as listed.
//
// GitHub is read once per pull request and the answer kept under
// .scratch/harness-lessons/<n>.json, so a second run, or one after the API
// quota is spent, costs nothing; a merged pull request's reviews do not change.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

// The review's own reading of the state its bodies carry.
import { parseState } from "./merge-review.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const PASS_SUBJECT = /^Harness: lessons from/;

// A stalled git or gh call ends after two minutes, as in the review and the gate.
function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: 2 * 60 * 1000 });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: [(r.stderr ?? "").trim(), r.error?.message].filter(Boolean).join("; ") };
}

// The body's "## Review" section: from that heading to the next "## ".
export function reviewSection(body) {
  const m = /^##\s+Review\s*$([\s\S]*?)(?=^##\s|(?![\s\S]))/m.exec(body ?? "");
  return m ? m[1].trim() : "";
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

const fileOf = (location) => (location ?? "").split(":")[0].split("/").pop().split(" ")[0];

// One pull request's comparison: the blocking findings the merge review posted,
// each with whether the author's Review section names its file. Only reviews
// by `author` (the account the merge review posts as) count: anyone can post a
// review with the marker on a public repository. A superseded review's
// findings count (the next review carries them as listed), the review itself
// is not a round.
export function compare(pr, author) {
  // Only a state of the shape the review writes: a body this account posted by
  // hand, or a truncated one, is not a review.
  const states = (pr.reviews ?? [])
    .filter((r) => r.author?.login === author)
    .map((r) => parseState(r.body))
    .filter((s) => typeof s?.head === "string" && Array.isArray(s.findings));
  const review = reviewSection(pr.body);
  if (!states.length) return { number: pr.number, title: pr.title, compared: false, why: "no merge review" };
  if (!review) return { number: pr.number, title: pr.title, compared: false, why: "no Review section in the body" };
  const seen = new Set();
  const findings = [];
  for (const s of states) {
    for (const f of s.findings) {
      if (!f || f.severity === "low" || f.origin !== "new") continue;
      const key = `${f.location}|${f.defect}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const file = fileOf(f.location);
      findings.push({ head: s.head.slice(0, 7), severity: f.severity, location: f.location, defect: f.defect, named: Boolean(file) && review.includes(file) });
    }
  }
  const rounds = states.filter((s) => !s.superseded);
  return { number: pr.number, title: pr.title, compared: true, rounds: rounds.length, blocked: rounds.filter((s) => s.verdict === "blocked").length, findings };
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

// Reads what one pull request needs, from the cache or GitHub. A cached file
// that does not parse (a write cut short) is read again from GitHub; the
// write goes through a rename, so a cut leaves no half file behind.
function fetchPullRequest(n, { run: r, cacheDir }) {
  const file = join(cacheDir, `${n}.json`);
  if (existsSync(file)) {
    try {
      return JSON.parse(readFileSync(file, "utf8"));
    } catch {}
  }
  const x = r("gh", ["pr", "view", String(n), "--json", "number,title,body,reviews"]);
  if (!x.ok) throw new Error(`gh pr view ${n} failed: ${x.err.split("\n")[0]}`);
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
  return { code: 0, stdout: `${report(results)}\n` };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const { code, stdout, stderr } = main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
