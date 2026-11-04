#!/usr/bin/env node
// The PR shepherd: moves every open pull request toward merge with no model
// session waiting. Each pull request gets one full review; a fix job works on
// what blocks it; a verify reads only the fix's commits; then it merges, or
// after three rounds goes to the owner with the `needs-owner` label. A script
// decides, and a model runs only inside a job (scripts/shepherd-job.mjs).
//
//   node scripts/shepherd.mjs --merge      run until everything is merged, closed or the owner's
//   node scripts/shepherd.mjs              shadow run: everything short of merging
//   node scripts/shepherd.mjs --dry-run    one pass that prints each decision and acts on none
//   --skip 117          leave these out (another session is driving them)
//   --reviewed 118=386eca0,...   record a clean review done elsewhere of that head
//   --max-jobs 10       claude -p jobs this run may start
//   --max-fixes 3       fix rounds per pull request for review findings (CI and conflicts count apart)
//   --interval 600      seconds between passes while nothing changes
//
// The set is the open pull requests at the start, minus drafts, forks and
// --skip. A pull request labeled `hold` is reviewed and fixed but never
// merged; one that changes CI, the shepherd, or the instructions agents run
// under is merged only by the owner (OWNER_MERGES). Each pull
// request carries one comment, kept up to date, with the latest findings and,
// when it needs the owner, why. A file at .scratch/pr-queue/stop ends the run
// at the next pass.
//
// Exit 0 when every pull request in the set is merged, closed, ready (in a
// shadow run) or the owner's; 3 when stopped or out of job budget; 2 on a tool
// error.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { killTree } from "./kill-tree.mjs";
import { commitsSinceReview } from "./merge-pr.mjs";
import { SHEPHERD_MARK } from "./pr-queue.mjs";
import { blocking, jobRunning, readLock } from "./shepherd-job.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const STATE = join(ROOT, ".scratch", "pr-queue");
const JOBS = join(STATE, "jobs");
const WORK = join(ROOT, ".scratch", "jobs");
const STOP = join(STATE, "stop");
const NEEDS_OWNER = "needs-owner";
const HOLD = "hold";

// Paths whose change the owner merges, so the loop never approves a change to
// its own judgement: CI and the check it runs, the shepherd and its gate, and
// the instructions, settings, MCP servers and hooks every agent (its reviewers
// included) runs under, in any folder.
export const OWNER_MERGES =
  /(^|\/)(CLAUDE(\.local)?\.md$|AGENTS\.md$|\.claude\/|\.mcp\.json$)|^(\.github\/|docs\/agents\/|scripts\/hooks\/|scripts\/(check|manifest|merge-pr|shepherd|shepherd-job|pr-queue|kill-tree)\.mjs$)/i;

// What the merge gate's dry run says: "merge" when it passes, "behind" when
// only main's tip is missing, "wait" otherwise (CI, or GitHub still computing).
export function readiness(code, out) {
  if (code === 0) return "merge";
  if (code === 1 && out.includes("does not contain main's tip") && !out.includes("review the head")) return "behind";
  return "wait";
}

// Whether a review record blocks the merge; records from the first loop kept a verdict.
export function blocks(r) {
  if (r.output?.verdict) return r.output.verdict !== "clean";
  return blocking(r.output?.findings).length > 0;
}

// What kind of work a fix record did, from its task.
export function fixKind(f) {
  if (/^CI failed/.test(f.task ?? "")) return "ci";
  if (/^GitHub reports this branch conflicts/.test(f.task ?? "")) return "conflict";
  return "review";
}

const byStart = (a, b) => a.startedAt.localeCompare(b.startedAt);

// Which review covers the head, and what the head still needs. `g` answers
// git questions: isAncestor(a, b), and onlyCleanMain(from, to), true when
// every commit after `from` is a clean merge of main.
//   none:   the latest review still covers the head
//   verify: the loop's own fix of review findings landed on the reviewed head; check only that
//   review: anything else (no review yet, a CI or conflict fix, or code no review or fix accounts for)
export function route(oid, reviews, fixes, g) {
  const p = reviews.filter((r) => g.isAncestor(r.head, oid)).sort(byStart).at(-1);
  if (!p) return { need: "review" };
  if (g.onlyCleanMain(p.head, oid)) return { need: "none", review: p };
  const f = fixes.filter((x) => x.head === p.head && x.fixKind === "review" && x.pushedHead && g.isAncestor(x.pushedHead, oid)).sort(byStart).at(-1);
  if (f && g.onlyCleanMain(f.pushedHead, oid)) return { need: "verify", review: p, fix: f };
  return { need: "review" };
}

// One action per pull request; `f` is what gather() found.
export function decide(row, f, { merge, maxFixes }) {
  const wait = (reason) => ({ kind: "wait", reason });
  const owner = (reason) => ({ kind: "owner", reason });
  const fix = (task, extra = {}) => {
    if (f.failed.fix >= 2) return owner("The automatic fixer crashed twice on this pull request. That points to a problem in the automation, not necessarily the code.");
    if (f.rounds[task] >= maxFixes) return owner(`The fixer has tried ${maxFixes} times and ${task === "review" ? "problems remain" : task === "ci" ? "the tests still fail" : "the conflict remains"}, so carrying on automatically is not working. Decide whether to merge anyway, fix it by hand, or close it.`);
    return { kind: "fix", task, ...extra };
  };

  if (row.state === "draft") return wait("draft");
  if (row.state === "ci-running") return wait("CI is running");
  if (f.dependabot) return owner("This is an automatic update to a library the project uses. Taking a new version is a policy call, so merge or close it yourself.");
  if (f.fixPending) return wait("GitHub does not show the fix's push yet");
  if (f.jobRunning) return wait("a job an earlier run started is still working on it");

  const mergeMain = () => {
    if (f.mainHere?.conflict) return f.fixHere("conflict") ? owner("This branch and main changed the same lines, and the fixer could not combine them. Someone needs to merge them by hand.") : fix("conflict");
    if ((f.mainHere?.failures ?? 0) >= 3) return owner(`Bringing this branch up to date with main failed three times, so it cannot be updated automatically. (${f.mainHere.error})`);
    return { kind: "merge-main" };
  };
  if (row.state === "conflicting") return mergeMain();

  // A rerun costs CI time and no tokens, and the self-hosted runners drop jobs.
  if (row.state === "ci-red") {
    if (!f.rerunHere) return { kind: "rerun" };
    if (row.shared) return owner(`The tests fail the same way on several pull requests, even after a rerun, so the cause is probably outside this one: the test machines, or main itself. (${row.signature})`);
    if (f.fixHere("ci")) return owner(`The tests still fail after an automatic fix attempt. (${row.signature})`);
    return fix("ci");
  }

  // green or behind-main
  const r = f.route;
  if (r.need !== "none") {
    if (f.failed.review >= 2) return owner("The automatic review crashed twice on this pull request. That points to a problem in the automation, not necessarily the code.");
    return r.need === "verify" ? { kind: "verify", review: r.review, fix: r.fix } : { kind: "review" };
  }
  if (blocks(r.review)) {
    const done = f.fixOn(r.review.head);
    if (done) return owner(`The automatic fixer did not fix this. ${done.output?.plain ?? (done.output?.summary ?? done.error ?? "").split("\n")[0].slice(0, 300)}`);
    return fix("review", { review: r.review });
  }
  if (f.ready === "behind") return mergeMain();
  if (f.ready !== "merge") return wait("the merge gate says not yet");
  if (f.hold) return { kind: "ready", reason: "labeled hold" };
  // A comment or review may ask for changes or say not to merge, and only a
  // person can tell which.
  if (row.feedback) return owner("People have commented on or reviewed this pull request, and the shepherd cannot tell whether they asked for changes or agree to the merge. Read the comments, then decide: merge it yourself, or ask for the changes they want.");
  if ((f.mergeRefused?.failures ?? 0) >= 2) return owner(`The merge checks pass, but GitHub refused the merge twice. A repository rule the shepherd does not check, such as a required approval, may need you. (${f.mergeRefused.error})`);
  if (f.ownerMerges) return owner("Ready to merge. It changes the automation's own rules (the test setup, the shepherd, or the instructions agents follow), so the shepherd will not approve it on its own: merge it yourself if you agree.");
  return merge ? { kind: "merge", review: r.review } : { kind: "ready", reason: "would merge (shadow run)" };
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

// The one comment the shepherd keeps on a pull request: plain English first,
// for the owner, and the technical detail folded below it.
export function notesComment(review, ownerReason) {
  const lines = [SHEPHERD_MARK];
  if (ownerReason) lines.push(`**Needs you:** ${ownerReason}`, "");
  const findings = review?.output?.findings ?? [];
  lines.push(!review ? "Not reviewed yet." : blocks(review) ? "**What blocks the merge:**" : findings.length ? "Nothing blocks the merge. Smaller follow-ups:" : "The review found no problems.");
  for (const x of findings) lines.push(`- ${x.plain ?? x.defect}`);
  if (findings.length) {
    lines.push("", `<details><summary>Technical detail (${review.since ? "verify" : "review"} of ${review.head.slice(0, 7)})</summary>`, "");
    for (const x of findings) lines.push(`- **${x.severity}**${x.origin === "listed" ? " (still open)" : ""}, \`${x.location}\`: ${x.defect} ${x.scenario}`);
    lines.push("", "</details>");
  }
  return `${lines.join("\n")}\n`;
}

// A command that stalls (a credential prompt, a network hang) is killed after
// ten minutes, with everything it started (pr-queue's gh and git, git's
// remote helpers), and reads as failed, so the loop never blocks on it.
function run(cmd, args, opts = {}) {
  const env = { ...process.env, GIT_TERMINAL_PROMPT: "0", GH_PROMPT_DISABLED: "1" };
  const since = new Date();
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, timeout: 10 * 60 * 1000, env, ...opts });
  if (r.error?.code === "ETIMEDOUT") killTree(r.pid, since);
  return { code: r.status, ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() || (r.error?.message ?? "") };
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

// A record torn by a killed job reads as missing, never as a crash of the loop.
const readJson = (f) => {
  try {
    return JSON.parse(readFileSync(f, "utf8"));
  } catch {
    return null;
  }
};
const jobFiles = () => (existsSync(JOBS) ? readdirSync(JOBS) : []);
const isAncestor = (a, b) => run("git", ["merge-base", "--is-ancestor", a, b]).ok;
const onlyCleanMain = (from, to) => {
  try {
    return commitsSinceReview({ reviewed: from, head: to }).every((c) => c.merge && c.fromMain && c.clean);
  } catch {
    return false;
  }
};

// The open pull requests, or null when GitHub could not be read: the caller
// decides whether that ends the run.
function queue() {
  const rows = run("node", ["scripts/pr-queue.mjs", "--json"]);
  if (!rows.ok) return (process.stderr.write(`pr-queue failed: ${rows.err}\n`), null);
  const prs = run("gh", ["pr", "list", "--state", "open", "--limit", "100", "--json", "number,headRefOid,headRefName,isCrossRepository,labels"]);
  if (!prs.ok) return (process.stderr.write(`gh pr list failed: ${prs.err}\n`), null);
  const info = new Map(JSON.parse(prs.out).map((p) => [p.number, { oid: p.headRefOid, branch: p.headRefName, fork: p.isCrossRepository, labels: p.labels.map((l) => l.name) }]));
  return JSON.parse(rows.out).flatMap((r) => (info.has(r.number) ? [{ ...r, ...info.get(r.number) }] : []));
}

// What the job records say about this pull request and its current head.
function gather(row, crashes = new Map()) {
  const id = (kind) => `${row.number}-${row.head}-${kind}`;
  const files = jobFiles();
  const mine = files.filter((f) => f.startsWith(`${row.number}-`) && /-(review|fix|fix-ci|fix-conflict)\.json$/.test(f)).map((f) => readJson(join(JOBS, f))).filter(Boolean);
  const reviews = mine.filter((r) => r.kind === "review");
  // A fix's kind comes from its task; first-loop records kept none, only a task file.
  const taskFile = (x, kind) => files.includes(`${row.number}-${x.head.slice(0, 7)}-${kind}.task.md`);
  const fixes = mine
    .filter((r) => r.kind === "fix")
    .map((x) => ({ ...x, fixKind: x.task ? fixKind(x) : taskFile(x, "ci") ? "ci" : taskFile(x, "conflict") ? "conflict" : "review" }));
  const failed = (kind) => files.filter((f) => isFailedRecord(f, row.number, kind)).length + (crashes.get(`${row.number}:${kind}`) ?? 0);
  const f = {
    dependabot: row.branch.startsWith("dependabot/"),
    hold: row.labels.includes(HOLD),
    jobRunning: files.some((x) => x.startsWith(`${row.number}-`) && x.endsWith(".lock") && jobRunning(readLock(join(JOBS, x)))),
    fixHere: (kind) => fixes.some((x) => x.head === row.oid && x.fixKind === kind),
    // A fix pushed, but GitHub still reports the head it started from.
    fixPending: fixes.some((x) => x.head === row.oid && x.pushedHead && x.pushedHead !== row.oid),
    fixOn: (head) => fixes.filter((x) => x.head === head && x.fixKind === "review").sort(byStart).at(-1),
    rounds: { review: 0, ci: 0, conflict: 0 },
    failed: { review: failed("review"), fix: failed("fix") },
    mainHere: files.includes(`${id("main")}.json`) ? readJson(join(JOBS, `${id("main")}.json`)) : null,
    mergeRefused: files.includes(`${id("refused")}.json`) ? readJson(join(JOBS, `${id("refused")}.json`)) : null,
    rerunHere: files.includes(`${id("rerun")}.json`),
    route: { need: "review" },
    ready: null,
    ownerMerges: false,
  };
  for (const x of fixes) f.rounds[x.fixKind]++;
  if (row.state === "green" || row.state === "behind-main") {
    f.route = route(row.oid, reviews, fixes, { isAncestor, onlyCleanMain });
    if (f.route.need === "none" && !blocks(f.route.review)) {
      const gate = run("node", ["scripts/merge-pr.mjs", String(row.number), "--reviewed", f.route.review.head, "--dry-run"]);
      f.ready = readiness(gate.code, gate.out);
      const diff = run("git", ["diff", "--name-only", "--no-renames", `origin/main...${row.oid}`]);
      // A diff that failed reads as touching owner paths, so the check fails closed.
      f.ownerMerges = !diff.ok || diff.out.split("\n").some((p) => OWNER_MERGES.test(p));
    }
  }
  return f;
}

function fresh(dir) {
  run("git", ["worktree", "remove", "--force", dir]);
  rmSync(dir, { recursive: true, force: true });
  run("git", ["worktree", "prune"]);
}

function mark(row, kind, data) {
  mkdirSync(JOBS, { recursive: true });
  writeFileSync(join(JOBS, `${row.number}-${row.head}-${kind}.json`), JSON.stringify({ at: new Date().toISOString(), ...data }, null, 2));
}

// Creates or updates the shepherd's one comment on a pull request.
function upsertComment(n, body) {
  const list = run("gh", ["api", `repos/{owner}/{repo}/issues/${n}/comments`, "--paginate"]);
  if (!list.ok) return `comment: ${list.err.split("\n")[0]}`;
  const mine = JSON.parse(list.out.replace(/\]\s*\[/g, ",") || "[]").find((c) => String(c.body).startsWith(SHEPHERD_MARK));
  const file = join(STATE, `comment-${n}.md`);
  writeFileSync(file, body);
  const r = mine
    ? run("gh", ["api", "-X", "PATCH", `repos/{owner}/{repo}/issues/comments/${mine.id}`, "-F", `body=@${file}`])
    : run("gh", ["api", `repos/{owner}/{repo}/issues/${n}/comments`, "-F", `body=@${file}`]);
  return r.ok ? "" : `comment: ${r.err.split("\n")[0]}`;
}

function latestReview(n) {
  return jobFiles()
    .filter((f) => f.startsWith(`${n}-`) && f.endsWith("-review.json"))
    .map((f) => readJson(join(JOBS, f)))
    .filter(Boolean)
    .sort(byStart)
    .at(-1);
}

// The cheap actions, done in the pass. Returns a line for the log.
function act(row, action) {
  const n = String(row.number);
  switch (action.kind) {
    case "merge-main": {
      const marker = join(JOBS, `${row.number}-${row.head}-main.json`);
      const failures = ((existsSync(marker) ? readJson(marker) : null)?.failures ?? 0) + 1;
      const failed = (error) => (mark(row, "main", { failures, error: error.split("\n")[0].slice(0, 300) }), `merge-main failed (${failures} of 3): ${error.split("\n")[0]}`);
      const dir = join(WORK, `${row.number}-${row.head}-main`);
      mkdirSync(WORK, { recursive: true });
      fresh(dir);
      const fetched = run("git", ["fetch", "-q", "origin", "main", `pull/${n}/head`]);
      if (!fetched.ok) return failed(`fetch: ${fetched.err}`);
      const added = run("git", ["worktree", "add", "--detach", dir, row.oid]);
      if (!added.ok) return failed(`worktree: ${added.err}`);
      const merged = run("git", ["merge", "--no-edit", "origin/main"], { cwd: dir });
      const conflicts = merged.ok ? "" : run("git", ["diff", "--name-only", "--diff-filter=U"], { cwd: dir }).out;
      if (!merged.ok) run("git", ["merge", "--abort"], { cwd: dir });
      const push = merged.ok ? run("git", ["push", "origin", `HEAD:${row.branch}`], { cwd: dir }) : null;
      fresh(dir);
      if (push?.ok) return "merged main in and pushed";
      if (!conflicts) return failed(push ? `push: ${push.err}` : `merge: ${merged.err || merged.out}`);
      mark(row, "main", { conflict: true, files: conflicts.split("\n") });
      return `merging main conflicts in ${conflicts.split("\n").join(", ")}; a fix job resolves it next`;
    }
    case "rerun": {
      const list = run("gh", ["run", "list", "--commit", row.oid, "--json", "databaseId,conclusion"]);
      if (!list.ok) return `rerun: gh run list failed: ${list.err.split("\n")[0]}`;
      const failed = JSON.parse(list.out || "[]").filter((r) => ["failure", "timed_out", "startup_failure", "cancelled"].includes(r.conclusion));
      const done = failed.filter((r) => run("gh", ["run", "rerun", String(r.databaseId), "--failed"]).ok).map((r) => r.databaseId);
      mark(row, "rerun", { runs: done });
      return `reran ${done.length} failed run(s)`;
    }
    case "merge": {
      const body = join(STATE, `merge-${n}.md`);
      const rv = action.review;
      writeFileSync(body, `Merged by the PR shepherd at ${row.oid.slice(0, 7)}: CI green on a head containing main; ${rv.since ? "verify" : "review"} of ${rv.head.slice(0, 7)} with nothing blocking (${(rv.output?.findings ?? []).length} follow-up findings).\n`);
      const m = run("node", ["scripts/merge-pr.mjs", n, "--reviewed", rv.head, "--body-file", body]);
      if (m.ok) return m.out;
      const error = (m.out || m.err).replace(/\n/g, " ");
      const marker = join(JOBS, `${row.number}-${row.head}-refused.json`);
      const failures = ((existsSync(marker) ? readJson(marker) : null)?.failures ?? 0) + 1;
      mark(row, "refused", { failures, error: error.slice(0, 300) });
      return `merge refused (${failures} of 2): ${error}`;
    }
    default:
      return action.reason;
  }
}

// Whether a file in the jobs folder records a failed job of this kind ("review"
// or "fix") on this pull request, on any head.
export function isFailedRecord(file, n, kind) {
  return file.startsWith(`${n}-`) && new RegExp(`-${kind}(-ci|-conflict)?\\.`).test(file) && file.endsWith(".failed.json");
}

// Whether a job crashed: it failed (any exit but 0, 3 for "already ran" or 4
// for "running now")
// and left no record of its own. Exit codes alone cannot tell: on Windows a
// killed job and an uncaught error both exit 1, like a job that failed and
// wrote its record. `before` and `after` list the jobs folder around the job.
// The loop counts crashes for the current run only, so a passing network
// blip never counts against a pull request for good.
export function crashed(code, before, after, n) {
  if (code === 0 || code === 3 || code === 4) return false;
  const old = new Set(before);
  return !after.some((f) => !old.has(f) && f.startsWith(`${n}-`) && f.endsWith(".json"));
}

// Starts a job without waiting for it; `done` settles with its exit code and last line.
function startJob(row, action) {
  const n = String(row.number);
  let args;
  if (action.kind === "review") args = ["review", n];
  if (action.kind === "verify") {
    const notes = join(JOBS, `${row.number}-${row.head}-verify.notes.md`);
    const fx = action.fix;
    const task = fx.task ?? (existsSync(join(JOBS, `${fx.pr}-${fx.head.slice(0, 7)}-review.task.md`)) ? readFileSync(join(JOBS, `${fx.pr}-${fx.head.slice(0, 7)}-review.task.md`), "utf8") : "");
    // The findings only: a reviewer never gets the author's own account (review-pr).
    writeFileSync(notes, `${task}\n`);
    args = ["review", n, "--since", action.review.head, "--notes", notes];
  }
  if (action.kind === "fix") {
    let task = join(JOBS, `${row.number}-${row.head}-${action.task}.task.md`);
    if (action.task === "ci") writeFileSync(task, ciTask(n, row.signature));
    if (action.task === "conflict") writeFileSync(task, CONFLICT_TASK);
    if (action.task === "review") task = join(JOBS, `${row.number}-${action.review.head.slice(0, 7)}-review.task.md`);
    args = ["fix", n, "--task", task];
  }
  const child = spawn("node", ["scripts/shepherd-job.mjs", ...args], { cwd: ROOT, stdio: ["ignore", "ignore", "pipe"] });
  let err = "";
  child.stderr.on("data", (d) => (err += d));
  return new Promise((settle) => {
    child.on("error", (e) => settle({ code: null, text: `failed to start: ${e.message}` }));
    child.on("close", (code) => settle({ code, text: code === 0 ? "done" : `failed (${code}): ${err.trim().split("\n").at(-1) ?? ""}` }));
  });
}

const JOB_KINDS = ["verify", "fix", "review"];
const TERMINAL = ["owner", "ready", "budget"];
const sleep = (s) => new Promise((r) => setTimeout(r, s * 1000));
const stamp = () => new Date().toISOString().slice(0, 19).replace("T", " ");
const labelOf = (a) => `${a.kind}${a.task ? ` (${a.task})` : ""}${a.reason ? `: ${a.reason}` : ""}`;

async function main() {
  const args = process.argv.slice(2);
  const opt = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
  const opts = {
    merge: args.includes("--merge"),
    dryRun: args.includes("--dry-run"),
    skip: new Set(String(opt("--skip", "")).split(",").filter(Boolean).map(Number)),
    maxJobs: Number(opt("--max-jobs", 10)),
    maxFixes: Number(opt("--max-fixes", 3)),
    interval: Number(opt("--interval", 600)),
  };
  if ([opts.maxJobs, opts.maxFixes, opts.interval].some((v) => !Number.isFinite(v))) fail("--max-jobs, --max-fixes and --interval take numbers");

  for (const pair of String(opt("--reviewed", "")).split(",").filter(Boolean)) {
    const [n, sha] = pair.split("=");
    const full = run("git", ["rev-parse", "--verify", "--quiet", `${sha}^{commit}`]).out;
    if (!/^\d+$/.test(n) || !full) fail(`--reviewed ${pair}: expected <n>=<sha> naming a commit here`);
    const file = join(JOBS, `${n}-${full.slice(0, 7)}-review.json`);
    if (!opts.dryRun) mkdirSync(JOBS, { recursive: true });
    if (!existsSync(file) && !opts.dryRun)
      writeFileSync(file, JSON.stringify({ kind: "review", pr: Number(n), head: full, since: null, blocks: false, startedAt: new Date().toISOString(), source: "--reviewed", output: { findings: [], dismissed: [] } }, null, 2));
  }
  if (!opts.dryRun) {
    run("gh", ["label", "create", NEEDS_OWNER, "--force", "--color", "D93F0B", "--description", "The PR shepherd stopped; its comment says why"]);
    run("gh", ["label", "create", HOLD, "--force", "--color", "FBCA04", "--description", "The PR shepherd reviews and fixes this but does not merge it"]);
  }

  const first = queue();
  if (!first) fail("could not read the pull requests");
  const set = new Set(first.filter((r) => r.state !== "draft" && !r.fork && !opts.skip.has(r.number)).map((r) => r.number));
  process.stdout.write(`${stamp()} set: ${[...set].map((n) => `#${n}`).join(" ") || "(empty)"}${opts.merge ? ", may merge" : ", shadow run"}\n`);
  const ctx = { jobs: 0, running: null, started: new Set(), crashes: new Map() };
  const said = new Map();
  const say = (n, state, what) => {
    if (said.get(n) === what) return;
    said.set(n, what);
    process.stdout.write(`${stamp()} #${n}\t${state}\t${what}\n`);
  };
  const posted = new Map();
  // The label and the comment say why a pull request waits on the owner.
  const flag = (row, action) => {
    if (opts.dryRun) return;
    const isOwner = action.kind === "owner";
    if (isOwner && posted.get(row.number) !== action.reason) {
      const e = upsertComment(row.number, notesComment(latestReview(row.number), action.reason));
      if (!e) posted.set(row.number, action.reason);
    }
    if (isOwner !== row.labels.includes(NEEDS_OWNER)) run("gh", ["pr", "edit", String(row.number), isOwner ? "--add-label" : "--remove-label", NEEDS_OWNER]);
    if (!isOwner) posted.delete(row.number);
  };

  for (;;) {
    if (existsSync(STOP)) {
      process.stdout.write(`${stamp()} stop requested (${STOP})${ctx.running ? `; waiting for the ${ctx.running.kind} on #${ctx.running.n} to finish` : ""}\n`);
      await ctx.running?.done;
      process.exit(3);
    }
    const all = queue();
    if (!all) {
      // A GitHub hiccup: try again next pass, and let a running job finish.
      await Promise.race([sleep(60), ctx.running?.done].filter(Boolean));
      continue;
    }
    const rows = all.filter((r) => set.has(r.number)).sort((a, b) => a.number - b.number);
    if (!rows.length && !ctx.running) {
      process.stdout.write(`${stamp()} done: every pull request in the set is merged or closed\n`);
      process.exit(0);
    }

    let merged = false;
    let acted = false;
    const lines = [];
    const wanting = [];
    for (const row of rows) {
      if (ctx.running?.n === row.number) {
        lines.push({ row, action: { kind: "running" }, what: `${ctx.running.kind} running` });
        continue;
      }
      let action = decide(row, gather(row, ctx.crashes), opts);
      if (action.kind === "merge" && merged) action = { kind: "wait", reason: "one merge per pass; main just moved" };
      if (JOB_KINDS.includes(action.kind)) {
        wanting.push({ row, action });
        continue;
      }
      let what = labelOf(action);
      if (!opts.dryRun && ["merge-main", "rerun", "merge"].includes(action.kind)) {
        say(row.number, row.state, `${what}: starting`);
        what = act(row, action);
        acted = true;
        merged ||= action.kind === "merge";
      }
      flag(row, action);
      say(row.number, row.state, what);
      lines.push({ row, action, what });
    }

    // At most one job at a time, the one nearest a merge first.
    wanting.sort((a, b) => JOB_KINDS.indexOf(a.action.kind) - JOB_KINDS.indexOf(b.action.kind) || a.row.number - b.row.number);
    for (const { row, action } of wanting) {
      let what = labelOf(action);
      const key = `${row.number}:${row.oid}:${what}`;
      if (ctx.started.has(key)) {
        // The same job on the same head twice in a run means its result did
        // not change the decision: a defect in the loop, not more work.
        action.reason = `The automation ran the same step (${what}) twice without making progress, which is a bug in the shepherd, not necessarily in this pull request.`;
        action.kind = "owner";
        what = `owner: ${action.reason}`;
      } else if (ctx.jobs >= opts.maxJobs) {
        what = `budget: would start a ${what}, but the run's ${opts.maxJobs} jobs are used`;
        action.kind = "budget";
      } else if (opts.dryRun || ctx.running) what = `${what} (queued)`;
      else {
        ctx.jobs++;
        ctx.started.add(key);
        const kind = labelOf(action);
        what = `${kind}: started (job ${ctx.jobs} of ${opts.maxJobs})`;
        const before = jobFiles();
        const done = startJob(row, action).then(({ code, text }) => {
          // Nothing here may stop the loop: an error is logged, and the slot is always freed.
          try {
            say(row.number, row.state, `${kind} ${text}`);
            if (crashed(code, before, jobFiles(), row.number)) {
              const c = `${row.number}:${action.kind === "fix" ? "fix" : "review"}`;
              ctx.crashes.set(c, (ctx.crashes.get(c) ?? 0) + 1);
            }
            // A job that failed may run again on this head; one that already ran (3) may not.
            if (code !== 0 && code !== 3) ctx.started.delete(key);
            if (action.kind !== "fix") {
              const rv = latestReview(row.number);
              if (rv) upsertComment(row.number, notesComment(rv));
            }
          } catch (e) {
            process.stderr.write(`${stamp()} after the ${kind} job on #${row.number}: ${e?.message ?? e}\n`);
          } finally {
            ctx.running = null;
          }
        });
        ctx.running = { n: row.number, kind, done };
        acted = true;
      }
      flag(row, action);
      say(row.number, row.state, what);
      lines.push({ row, action, what });
    }

    mkdirSync(STATE, { recursive: true });
    writeFileSync(join(STATE, "report.md"), `${lines.map(({ row, what }) => `#${row.number}\t${row.state}\t${what}`).join("\n")}\n`);

    if (opts.dryRun) process.exit(0);
    if (!ctx.running && lines.every((l) => TERMINAL.includes(l.action.kind))) {
      const outOfBudget = lines.some((l) => l.action.kind === "budget");
      process.stdout.write(`${stamp()} ${outOfBudget ? "out of job budget" : "done: everything left is ready or waits on you"}\n`);
      process.exit(outOfBudget ? 3 : 0);
    }
    // A finished job wakes the loop at once; otherwise poll often while work is
    // in flight, and rarely when nothing is.
    await Promise.race([sleep(acted || ctx.running ? 60 : opts.interval), ctx.running?.done].filter(Boolean));
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
