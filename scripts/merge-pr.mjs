#!/usr/bin/env node
// The merge gate: merges one pull request only when the merge rule in
// docs/agents/orchestration.md holds, so whoever calls it (the owner, an
// orchestration session, the PR shepherd) cannot merge on a misread.
//
//   node scripts/merge-pr.mjs <n> --reviewed <sha>            merge if the gate passes
//   node scripts/merge-pr.mjs <n> --reviewed <sha> --dry-run  say whether it would
//
// The gate: the pull request is open, not a draft, and mergeable; every latest
// CI check passed; the head contains main's tip; and the head is the reviewed
// commit, or differs from it only by clean merges of main. A merge of main that
// resolved a conflict by hand changes code no reviewer saw, so it needs a new
// review. The merge names the head it checked (--match-head-commit), so a push
// between the check and the merge makes GitHub refuse it.
//
// Exit 0 when merged (or, with --dry-run, when it would merge); 1 when the gate
// refuses, with one line per reason; 2 on a usage or tool error.

import { spawnSync } from "node:child_process";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { latestChecks } from "./pr-queue.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const PASSED = ["SUCCESS", "SKIPPED", "NEUTRAL"];

// Why the pull request may not merge; empty when it may. `sinceReview` lists
// the commits on the branch after the reviewed one that main does not have.
export function refusals({ pr, containsMain, reviewedInBranch, sinceReview }) {
  const out = [];
  if (pr.state !== "OPEN") out.push(`it is ${pr.state.toLowerCase()}`);
  if (pr.isDraft) out.push("it is a draft");
  if (pr.mergeable !== "MERGEABLE") out.push(`GitHub reports it ${pr.mergeable.toLowerCase()}`);

  // A commit status (rather than a check run) carries `context` and `state`.
  const checks = latestChecks(
    (pr.statusCheckRollup ?? []).map((c) => ({ ...c, name: c.name ?? c.context, status: c.status ?? (["PENDING", "EXPECTED"].includes(c.state) ? "PENDING" : "COMPLETED"), conclusion: c.conclusion ?? c.state }))
  );
  const running = checks.filter((c) => c.status !== "COMPLETED");
  const failed = checks.filter((c) => c.status === "COMPLETED" && !PASSED.includes(c.conclusion));
  if (!checks.length) out.push("it has no CI checks");
  if (running.length) out.push(`CI is still running: ${running.map((c) => c.name).join(", ")}`);
  if (failed.length) out.push(`CI did not pass: ${failed.map((c) => c.name).join(", ")}`);
  if (!containsMain) out.push("its head does not contain main's tip; merge main in and let CI run");

  if (!reviewedInBranch) out.push("the reviewed commit is not in the branch's history; review the head");
  for (const c of sinceReview ?? []) {
    const short = c.sha.slice(0, 7);
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

class Refused extends Error {}
class ToolError extends Error {}
const fail = (message) => {
  throw new ToolError(message);
};

const USAGE = "usage: node scripts/merge-pr.mjs <n> --reviewed <sha> [--dry-run]";

// Runs the gate for argv and returns the exit code and what to print. `run`
// runs one git or gh command, so a test can stand in for GitHub.
export function main(argv, { run: r = run } = {}) {
  try {
    return { code: 0, ...gate(argv, r) };
  } catch (e) {
    if (e instanceof Refused) return { code: 1, stdout: e.message };
    return { code: 2, stderr: `${e.message}\n` };
  }
}

function gate(args, run) {
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

  const view = run("gh", ["pr", "view", n, "--json", "state,isDraft,mergeable,headRefOid,statusCheckRollup"]);
  if (!view.ok) fail(`gh pr view failed: ${view.err}`);
  let pr;
  try {
    pr = JSON.parse(view.out);
  } catch (e) {
    fail(`gh pr view printed no JSON: ${e.message}`);
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

  const reasons = refusals({ pr, containsMain: isAncestor("origin/main", head), reviewedInBranch, sinceReview });
  if (reasons.length) throw new Refused(`#${n} may not merge:\n${reasons.map((x) => `- ${x}\n`).join("")}`);
  if (dryRun) return { stdout: `#${n} may merge at ${head.slice(0, 7)}\n` };
  const merge = run("gh", ["pr", "merge", n, "--merge", "--match-head-commit", head]);
  if (!merge.ok) fail(`gh pr merge failed: ${merge.err}`);
  return { stdout: `#${n} merged at ${head.slice(0, 7)}\n` };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { code, stdout, stderr } = main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
