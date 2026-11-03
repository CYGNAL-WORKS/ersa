#!/usr/bin/env node
// The PR shepherd: moves a set of open pull requests toward merge with no model
// session waiting. Each pass reads the queue (scripts/pr-queue.mjs), decides
// one action per pull request, and acts: merges main into a branch, reruns a
// flaky CI run, starts a review or a fix (scripts/shepherd-job.mjs, a headless
// claude -p run), or merges through the gate (scripts/merge-pr.mjs). Between
// passes it sleeps; a model runs only inside a job.
//
//   node scripts/shepherd.mjs              run until the set is done, waiting on you, or out of budget
//   node scripts/shepherd.mjs --dry-run    one pass that prints each decision and acts on none
//   --merge             may merge pull requests the gate passes (otherwise they wait on you)
//   --skip 31,117       leave these out of the set (another session is driving them, say)
//   --reviewed 118=386eca0,...   record a clean review done elsewhere of that head
//   --max-jobs 8        claude -p jobs this run may start
//   --max-fixes 2       fix jobs per pull request, across heads
//   --interval 600      seconds between passes while nothing changes
//
// The set is the open pull requests at the start, minus drafts and --skip. A
// file at .scratch/pr-queue/stop ends the run at the next pass. Each pass
// writes .scratch/pr-queue/report.md, one line per pull request, and prints it
// when it changed.
//
// Exit 0 when every pull request in the set is merged, closed, or waiting on a
// decision only the owner can make; 3 when stopped or out of job budget; 2 on a
// tool error.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";


const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const STATE = join(ROOT, ".scratch", "pr-queue");
const JOBS = join(STATE, "jobs");
const WORK = join(ROOT, ".scratch", "jobs");
const STOP = join(STATE, "stop");

// What the merge gate's dry run says about a pull request with a clean review:
// "merge" when it passes, "review" when the head changed in a way no reviewer
// saw, "behind" when only main's tip is missing, "wait" otherwise (CI, or
// GitHub still computing mergeability).
export function readiness(code, out) {
  if (code === 0) return "merge";
  if (code !== 1) return "wait";
  if (out.includes("review the head")) return "review";
  if (out.includes("does not contain main's tip")) return "behind";
  return "wait";
}

// One action per pull request. `f` holds what the job records say about the
// current head; see gather().
export function decide(row, f, { merge, maxFixes }) {
  const wait = (reason) => ({ kind: "wait", reason });
  const owner = (reason) => ({ kind: "owner", reason });
  const fixOr = (task, ownerReason) =>
    !f.fixHere && f.fixes < maxFixes && f.failed.fix < 2 ? { kind: "fix", task } : owner(ownerReason);

  if (row.state === "draft") return wait("draft");
  if (row.state === "ci-running") return wait("CI is running");
  if (f.dependabot) return owner("a dependency update: yours to take or close");

  if (row.state === "conflicting") {
    if (!f.mainMergedHere) return { kind: "merge-main" };
    return fixOr("conflict", `conflicts with main after ${f.fixes} fix rounds`);
  }

  // A rerun costs CI time and no tokens, and the self-hosted runners drop jobs,
  // so every red head gets one before a fix job reads its log.
  if (row.state === "ci-red") {
    if (!f.rerunHere) return { kind: "rerun" };
    if (row.shared) return owner(`CI fails the same way on several pull requests, after a rerun: ${row.signature}`);
    return fixOr("ci", `CI still red after a rerun and ${f.fixes} fix rounds: ${row.signature}`);
  }

  // green or behind-main
  if (f.ready === "merge") return merge ? { kind: "merge" } : owner("reviewed clean and ready: merge it, or run with --merge");
  if (f.ready === "behind") return f.mainMergedHere ? owner("merging main in did not land") : { kind: "merge-main" };
  if (f.ready === "wait") return wait("the merge gate says not yet");
  if (!f.reviewHere) return f.failed.review < 2 ? { kind: "review" } : owner("the review job failed twice on this head");
  if (f.reviewHere.verdict === "clean") return wait("reviewed clean; the gate will decide next pass");
  return fixOr("review", `review findings remain after ${f.fixes} fix rounds`);
}

export function ciTask(n, signature) {
  return [
    `CI failed on this head: ${signature}.`,
    `Read the failing job's log (\`gh pr checks ${n}\`, then \`gh run view <run id> --log-failed\`), find the cause, and fix it.`,
    "If the failure is not this branch's doing (a runner, main, a flaky test elsewhere), change nothing and say so.",
  ].join(" ");
}

export const CONFLICT_TASK = [
  "GitHub reports this branch conflicts with main. Merge it: `git fetch origin main`, then `git merge origin/main`.",
  "Resolve each conflict keeping both sides' intent, run `git diff --cached --check` (it fails on a conflict marker left behind), and commit the merge.",
].join(" ");

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, ...opts });
  return { code: r.status, ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() || (r.error?.message ?? "") };
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

const readJson = (f) => JSON.parse(readFileSync(f, "utf8"));
const jobFiles = () => (existsSync(JOBS) ? readdirSync(JOBS) : []);

function queue() {
  const rows = run("node", ["scripts/pr-queue.mjs", "--json"]);
  if (!rows.ok) fail(`pr-queue failed: ${rows.err}`);
  const prs = run("gh", ["pr", "list", "--state", "open", "--limit", "100", "--json", "number,headRefOid,headRefName"]);
  if (!prs.ok) fail(`gh pr list failed: ${prs.err}`);
  const info = new Map(JSON.parse(prs.out).map((p) => [p.number, { oid: p.headRefOid, branch: p.headRefName }]));
  return JSON.parse(rows.out).flatMap((r) => (info.has(r.number) ? [{ ...r, ...info.get(r.number) }] : []));
}

// What the job records say about this pull request's current head.
function gather(row) {
  const id = (kind) => `${row.number}-${row.head}-${kind}`;
  const files = jobFiles();
  const mine = files.filter((f) => f.startsWith(`${row.number}-`) && /-(review|fix)\.json$/.test(f)).map((f) => readJson(join(JOBS, f)));
  const reviewHere = files.includes(`${id("review")}.json`) ? readJson(join(JOBS, `${id("review")}.json`)) : null;

  let ready = null;
  if (row.state === "green" || row.state === "behind-main") {
    const clean = mine
      .filter((r) => r.kind === "review" && r.output?.verdict === "clean")
      .filter((r) => run("git", ["merge-base", "--is-ancestor", r.head, row.oid]).ok)
      .sort((a, b) => b.startedAt.localeCompare(a.startedAt))[0];
    if (clean) {
      const gate = run("node", ["scripts/merge-pr.mjs", String(row.number), "--reviewed", clean.head, "--dry-run"]);
      ready = readiness(gate.code, gate.out);
      row.reviewed = clean.head;
    }
  }
  return {
    dependabot: row.branch.startsWith("dependabot/"),
    reviewHere,
    fixHere: files.includes(`${id("fix")}.json`),
    fixes: mine.filter((r) => r.kind === "fix").length,
    failed: {
      review: files.filter((f) => f.startsWith(`${id("review")}.`) && f.endsWith(".failed.json")).length,
      fix: files.filter((f) => f.startsWith(`${id("fix")}.`) && f.endsWith(".failed.json")).length,
    },
    mainMergedHere: files.includes(`${id("main")}.json`),
    rerunHere: files.includes(`${id("rerun")}.json`),
    ready,
  };
}

function mark(row, kind, data) {
  mkdirSync(JOBS, { recursive: true });
  writeFileSync(join(JOBS, `${row.number}-${row.head}-${kind}.json`), JSON.stringify({ at: new Date().toISOString(), ...data }, null, 2));
}

function job(args) {
  const r = run("node", ["scripts/shepherd-job.mjs", ...args], { stdio: ["ignore", "pipe", "pipe"] });
  return r.ok ? "done" : `failed (${r.code}): ${(r.err || r.out).split("\n").at(-1)}`;
}

// Returns a line for the log; `ctx.jobs` counts claude -p runs.
function act(row, action, ctx) {
  const n = String(row.number);
  switch (action.kind) {
    case "review":
      ctx.jobs++;
      return `review ${job(["review", n])}`;
    case "fix": {
      ctx.jobs++;
      const task = join(JOBS, `${row.number}-${row.head}-${action.task}.task.md`);
      if (action.task === "ci") writeFileSync(task, ciTask(n, row.signature));
      if (action.task === "conflict") writeFileSync(task, CONFLICT_TASK);
      return `fix (${action.task}) ${job(["fix", n, "--task", task])}`;
    }
    case "merge-main": {
      const dir = join(WORK, `${row.number}-${row.head}-main`);
      mkdirSync(WORK, { recursive: true });
      run("git", ["fetch", "-q", "origin", "main", `pull/${n}/head`]);
      if (!run("git", ["worktree", "add", "--detach", dir, row.oid]).ok) return "merge-main: could not make a worktree";
      const merged = run("git", ["merge", "--no-edit", "origin/main"], { cwd: dir });
      if (!merged.ok) run("git", ["merge", "--abort"], { cwd: dir });
      const push = merged.ok ? run("git", ["push", "origin", `HEAD:${row.branch}`], { cwd: dir }) : null;
      run("git", ["worktree", "remove", "--force", dir]);
      mark(row, "main", { merged: merged.ok, pushed: !!push?.ok, error: push && !push.ok ? push.err.slice(0, 300) : null });
      if (merged.ok) return push.ok ? "merged main in and pushed" : `merged main in, push failed: ${push.err.split("\n")[0]}`;
      // A conflict git could not resolve goes straight to a fix job.
      return `merging main conflicts; ${act(row, { kind: "fix", task: "conflict" }, ctx)}`;
    }
    case "rerun": {
      const runs = JSON.parse(run("gh", ["run", "list", "--commit", row.oid, "--json", "databaseId,conclusion"]).out || "[]");
      const failed = runs.filter((r) => ["failure", "timed_out", "startup_failure", "cancelled"].includes(r.conclusion));
      const done = failed.filter((r) => run("gh", ["run", "rerun", String(r.databaseId), "--failed"]).ok).map((r) => r.databaseId);
      mark(row, "rerun", { runs: done });
      return `reran ${done.length} failed run(s)`;
    }
    case "merge": {
      ctx.merged = true;
      const m = run("node", ["scripts/merge-pr.mjs", n, "--reviewed", row.reviewed]);
      return m.ok ? m.out : `merge refused: ${(m.out || m.err).replace(/\n/g, " ")}`;
    }
    default:
      return action.reason;
  }
}

const JOB_KINDS = ["review", "fix"];
const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000));
const stamp = () => new Date().toISOString().slice(0, 19).replace("T", " ");

async function main() {
  const args = process.argv.slice(2);
  const opt = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
  const opts = {
    merge: args.includes("--merge"),
    dryRun: args.includes("--dry-run"),
    skip: new Set(String(opt("--skip", "")).split(",").filter(Boolean).map(Number)),
    maxJobs: Number(opt("--max-jobs", 8)),
    maxFixes: Number(opt("--max-fixes", 2)),
    interval: Number(opt("--interval", 600)),
  };
  if ([opts.maxJobs, opts.maxFixes, opts.interval].some((v) => !Number.isFinite(v))) fail("--max-jobs, --max-fixes and --interval take numbers");

  // A clean review done elsewhere (a session, the owner) counts as a job record.
  for (const pair of String(opt("--reviewed", "")).split(",").filter(Boolean)) {
    const [n, sha] = pair.split("=");
    const full = run("git", ["rev-parse", "--verify", "--quiet", `${sha}^{commit}`]).out;
    if (!/^\d+$/.test(n) || !full) fail(`--reviewed ${pair}: expected <n>=<sha> naming a commit here`);
    const file = join(JOBS, `${n}-${full.slice(0, 7)}-review.json`);
    mkdirSync(JOBS, { recursive: true });
    if (!existsSync(file))
      writeFileSync(file, JSON.stringify({ kind: "review", pr: Number(n), head: full, startedAt: new Date().toISOString(), source: "--reviewed", output: { verdict: "clean", findings: [], dismissed: [] } }, null, 2));
  }

  const set = new Set(queue().filter((r) => r.state !== "draft" && !opts.skip.has(r.number)).map((r) => r.number));
  process.stdout.write(`${stamp()} set: ${[...set].map((n) => `#${n}`).join(" ") || "(empty)"}${opts.merge ? ", may merge" : ""}\n`);
  const ctx = { jobs: 0, merged: false };
  // Each pull request's line prints when it changes, and an action prints as
  // it starts, so the log shows what is running now, not only finished passes.
  const said = new Map();
  const say = (row, what) => {
    if (said.get(row.number) === what) return;
    said.set(row.number, what);
    process.stdout.write(`${stamp()} #${row.number}\t${row.state}\t${what}\n`);
  };

  for (;;) {
    if (existsSync(STOP)) {
      process.stdout.write(`${stamp()} stop requested (${STOP})\n`);
      process.exit(3);
    }
    const rows = queue().filter((r) => set.has(r.number)).sort((a, b) => a.number - b.number);
    if (!rows.length) {
      process.stdout.write(`${stamp()} done: every pull request in the set is merged or closed\n`);
      process.exit(0);
    }

    ctx.merged = false;
    let acted = false;
    const lines = [];
    for (const row of rows) {
      let action = decide(row, gather(row), opts);
      if (action.kind === "merge" && ctx.merged) action = { kind: "wait", reason: "one merge per pass; main just moved" };
      if (JOB_KINDS.includes(action.kind) && ctx.jobs >= opts.maxJobs) action = { kind: "budget", reason: `would start a ${action.kind}, but the run's ${opts.maxJobs} jobs are used` };
      const label = `${action.kind}${action.task ? ` (${action.task})` : ""}${action.reason ? `: ${action.reason}` : ""}`;
      const acts = !opts.dryRun && !["wait", "owner", "budget"].includes(action.kind);
      if (acts) {
        say(row, `${label}: starting${JOB_KINDS.includes(action.kind) ? ` (job ${ctx.jobs + 1} of ${opts.maxJobs})` : ""}`);
        acted = true;
      }
      const what = acts ? act(row, action, ctx) : label;
      say(row, what);
      lines.push({ row, action, what });
    }

    mkdirSync(STATE, { recursive: true });
    writeFileSync(join(STATE, "report.md"), `${lines.map(({ row, what }) => `#${row.number}\t${row.state}\t${what}`).join("\n")}\n`);

    if (opts.dryRun) process.exit(0);
    // Nothing left that a pass could move: the rest is the owner's, or needs a job the budget has no room for.
    if (lines.every((l) => ["owner", "budget"].includes(l.action.kind))) {
      const outOfBudget = lines.some((l) => l.action.kind === "budget");
      process.stdout.write(`${stamp()} ${outOfBudget ? "out of job budget" : "done: everything left waits on you"}\n`);
      process.exit(outOfBudget ? 3 : 0);
    }
    // After an action GitHub needs a moment to start CI or settle mergeability.
    await sleep(acted ? 60 : opts.interval);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
