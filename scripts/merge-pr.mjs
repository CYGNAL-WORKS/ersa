#!/usr/bin/env node
// The merge gate: adds one pull request to the merge queue only when the merge
// rule in docs/agents/orchestration.md holds, so whoever calls it (the owner,
// an orchestration session, the merge review) cannot merge on a misread.
//
//   node scripts/merge-pr.mjs <n> --reviewed <sha>            enqueue if the gate passes
//   node scripts/merge-pr.mjs <n> --reviewed <sha> --dry-run  say whether it would
//
// The gate: the pull request is open, not a draft, and mergeable; every latest
// CI check passed; the head's docs/briefs/INBOX.md has no open entry, since an
// entry is a brief gap the branch's code depends on and waits for the planning
// commit that closes it (ADR 0025); and the head is the reviewed commit, or
// differs from it only by clean merges of main. A merge of main that resolved a conflict by hand
// changes code no reviewer saw, so it needs a new review. The head need not
// contain main's tip: the merge queue builds and tests the merge before main
// moves (ADR 0023). The enqueue names the head it checked (--match-head-commit),
// so a push between the check and the enqueue makes GitHub refuse it.
//
// Exit 0 when added to the queue (or, with --dry-run, when it would be); 1
// when the gate refuses, with one line per reason; 2 on a usage or tool error.

import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const PASSED = ["SUCCESS", "SKIPPED", "NEUTRAL"];

// The merge review's own commit status on a head. It is not CI, so the gate
// never counts or waits for it: a head with only that status has not been built.
export const MERGE_REVIEW = "Merge review";

// The latest run of each check by name: a head can carry a run that a newer
// one superseded and cancelled, and only the newer one counts.
export function latestChecks(checks) {
  const latest = new Map();
  for (const c of checks) {
    const seen = latest.get(c.name);
    if (!seen || (c.startedAt ?? "") > (seen.startedAt ?? "")) latest.set(c.name, c);
  }
  return [...latest.values()];
}

// gh's statusCheckRollup in one shape: a commit status (rather than a check
// run) carries `context` and `state`, a check run `name`, `status` and `conclusion`.
export const normalizeChecks = (rollup) =>
  latestChecks(
    (rollup ?? []).map((c) => ({ ...c, name: c.name ?? c.context, status: c.status ?? (["PENDING", "EXPECTED"].includes(c.state) ? "PENDING" : "COMPLETED"), conclusion: c.conclusion ?? c.state }))
  );

// Why a head's CI does not count as passed; empty when every latest check
// finished and passed. The merge review poller asks this too, so the two read
// CI the same way.
export function ciRefusals(rollup) {
  const checks = normalizeChecks(rollup).filter((c) => c.name !== MERGE_REVIEW);
  const out = [];
  const running = checks.filter((c) => c.status !== "COMPLETED");
  const failed = checks.filter((c) => c.status === "COMPLETED" && !PASSED.includes(c.conclusion));
  if (!checks.length) out.push("it has no CI checks");
  if (running.length) out.push(`CI is still running: ${running.map((c) => c.name).join(", ")}`);
  if (failed.length) out.push(`CI did not pass: ${failed.map((c) => c.name).join(", ")}`);
  return out;
}

// A commit after the review that changes nothing a reviewer read: a merge of
// main with no hand-resolved conflict. The merge review reads it the same way.
export const cleanMergeOfMain = (c) => c.merge && c.fromMain && c.clean;

// How many open entries (`- [ ]` lines) an inbox file's text holds.
export const openInboxEntries = (text) => text.split(/\r?\n/).filter((l) => /^\s*-\s*\[ \]/.test(l)).length;

// Why the pull request may not merge; empty when it may. `sinceReview` lists
// the commits on the branch after the reviewed one that main does not have;
// `openInbox` counts the head's open inbox entries.
export function refusals({ pr, reviewedInBranch, sinceReview, openInbox = 0 }) {
  const out = [];
  if (openInbox) out.push(`${openInbox} open inbox entr${openInbox === 1 ? "y needs" : "ies need"} a planning pass`);
  if (pr.state !== "OPEN") out.push(`it is ${pr.state.toLowerCase()}`);
  if (pr.isDraft) out.push("it is a draft");
  if (pr.mergeable !== "MERGEABLE") out.push(`GitHub reports it ${pr.mergeable.toLowerCase()}`);
  out.push(...ciRefusals(pr.statusCheckRollup));
  // The review's own verdict on the head: red means findings or a failed run,
  // and a run by hand must not enqueue past it. Pending is the review itself,
  // which calls this gate before it finishes.
  const review = normalizeChecks(pr.statusCheckRollup).find((c) => c.name === MERGE_REVIEW);
  if (review && ["FAILURE", "ERROR"].includes(review.conclusion)) out.push("the merge review blocks the merge; see the review on the pull request");

  if (!reviewedInBranch) out.push("the reviewed commit is not in the branch's history; review the head");
  for (const c of sinceReview ?? []) {
    const short = c.sha.slice(0, 7);
    if (cleanMergeOfMain(c)) continue;
    if (!c.merge) out.push(`${short}, after the review, is not a merge; review the head`);
    else if (!c.fromMain) out.push(`${short}, after the review, merges a branch other than main; review the head`);
    else if (!c.clean) out.push(`${short}, after the review, resolved a conflict by hand; review the head`);
  }
  return out;
}

// A git or gh command that stalls (a credential prompt, a network hang) is
// killed after TIMEOUT_MS and reads as failed, so the gate never blocks.
const TIMEOUT_MS = 2 * 60 * 1000;

function run(cmd, args, cwd = ROOT) {
  const r = spawnSync(cmd, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: TIMEOUT_MS,
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0", GH_PROMPT_DISABLED: "1" },
  });
  const err = [(r.stderr ?? "").trim(), r.error?.message].filter(Boolean).join("; ");
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err };
}

// The commits on `head` after `reviewed` that `main` does not have, each
// classified for refusals(). `git` runs one git command and returns run()'s
// shape. A git command that fails throws, so a failure never reads as an empty
// list or a clean merge.
export function commitsSinceReview({ reviewed, head, main = "origin/main", git = (args) => run("git", args) }) {
  const must = (args) => {
    const r = git(args);
    if (!r.ok) throw new Error(`git ${args.join(" ")} failed: ${r.err}`);
    return r.out;
  };
  // A failure here reads as "not from main", which refuses: safe as is.
  const isMain = (p) => git(["merge-base", "--is-ancestor", p, main]).ok;
  return must(["rev-list", "--parents", `${reviewed}..${head}`, "--not", main])
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const [sha, , ...others] = line.split(" ");
      const merge = others.length > 0;
      return {
        sha,
        merge,
        fromMain: merge && others.every(isMain),
        // git skips the remerge-diff of an octopus merge and prints nothing,
        // so only a two-parent merge can read as clean.
        clean: others.length === 1 && must(["show", "--remerge-diff", "--format=", sha]) === "",
      };
    });
}

class Refused extends Error {
  constructor(n, reasons) {
    super(`#${n} may not merge:\n${reasons.map((x) => `- ${x}\n`).join("")}`);
    this.reasons = reasons;
  }
}
class ToolError extends Error {}
const fail = (message) => {
  throw new ToolError(message);
};

const USAGE = "usage: node scripts/merge-pr.mjs <n> --reviewed <sha> [--dry-run]";

const pause = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);

// Runs the gate for argv and returns the exit code, what to print and, when
// the gate refuses, its reasons. `run` runs one git or gh command and `wait`
// pauses, so a test can stand in for GitHub and the clock.
export function main(argv, { run: r = run, wait = pause } = {}) {
  try {
    return { code: 0, ...gate(argv, r, wait) };
  } catch (e) {
    if (e instanceof Refused) return { code: 1, stdout: e.message, reasons: e.reasons };
    return { code: 2, stderr: `${e.message}\n` };
  }
}

function gate(args, run, wait) {
  let n;
  let reviewedArg;
  let dryRun = false;
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "--reviewed" && reviewedArg === undefined && i + 1 < args.length) reviewedArg = args[++i];
    else if (a === "--dry-run") dryRun = true;
    else if (/^\d+$/.test(a) && n === undefined) n = a;
    else fail(`unrecognized argument ${a}\n${USAGE}`);
  }
  if (!n || !reviewedArg) fail(USAGE);

  const view = () => {
    const r = run("gh", ["pr", "view", n, "--json", "state,isDraft,mergeable,headRefOid,statusCheckRollup"]);
    if (!r.ok) fail(`gh pr view failed: ${r.err}`);
    try {
      return JSON.parse(r.out);
    } catch (e) {
      fail(`gh pr view printed no JSON: ${e.message}`);
    }
  };
  let pr = view();
  // GitHub works out whether a fresh head merges cleanly when first asked and
  // answers UNKNOWN meanwhile; one more read after a pause usually has it.
  if (pr?.mergeable === "UNKNOWN") {
    wait(5000);
    pr = view();
  }
  const head = pr?.headRefOid;
  if (typeof head !== "string" || typeof pr.state !== "string" || typeof pr.mergeable !== "string") fail("gh pr view left out state, mergeable or headRefOid");

  const fetch = run("git", ["fetch", "-q", "origin", "main", `pull/${n}/head`]);
  if (!fetch.ok) fail(`git fetch failed: ${fetch.err}`);
  const reviewed = run("git", ["rev-parse", "--verify", "--quiet", `${reviewedArg}^{commit}`]);
  if (!reviewed.ok) fail(`--reviewed ${reviewedArg} names no commit here`);

  const isAncestor = (a, b) => run("git", ["merge-base", "--is-ancestor", a, b]).ok;
  const reviewedInBranch = isAncestor(reviewed.out, head);
  const sinceReview = reviewedInBranch ? commitsSinceReview({ reviewed: reviewed.out, head, git: (a) => run("git", a) }) : [];

  // The head's inbox, read from git: ls-tree prints nothing when the file is
  // absent, and a git failure is a tool error, so it never reads as empty.
  const INBOX = "docs/briefs/INBOX.md";
  const listed = run("git", ["ls-tree", head, "--", INBOX]);
  if (!listed.ok) fail(`git ls-tree failed: ${listed.err}`);
  let openInbox = 0;
  if (listed.out) {
    const inbox = run("git", ["show", `${head}:${INBOX}`]);
    if (!inbox.ok) fail(`git show ${INBOX} failed: ${inbox.err}`);
    openInbox = openInboxEntries(inbox.out);
  }

  const reasons = refusals({ pr, reviewedInBranch, sinceReview, openInbox });
  if (reasons.length) throw new Refused(n, reasons);
  if (dryRun) return { stdout: `#${n} may enter the merge queue at ${head.slice(0, 7)}\n` };
  // --auto on a branch with a merge queue adds the pull request to the queue,
  // or arms it until its required checks pass; the queue sets the merge
  // method, so --merge only states the intent gh would otherwise ask for.
  const merge = run("gh", ["pr", "merge", n, "--auto", "--merge", "--match-head-commit", head]);
  if (!merge.ok) fail(`gh pr merge failed: ${merge.err}`);
  return { stdout: `#${n} added to the merge queue at ${head.slice(0, 7)}\n` };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { code, stdout, stderr } = main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
