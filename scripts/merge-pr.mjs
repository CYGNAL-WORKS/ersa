#!/usr/bin/env node
// The merge gate: merges one pull request only when the merge rule in
// docs/agents/orchestration.md holds, so whoever calls it (the owner, an
// orchestration session, the PR shepherd) cannot merge on a misread.
//
//   node scripts/merge-pr.mjs <n> --reviewed <sha>            merge if the gate passes
//   node scripts/merge-pr.mjs <n> --reviewed <sha> --dry-run  say whether it would
//   ... --body-file <file>    the merge commit's body: what the merge rests on
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

function run(cmd, args, cwd = ROOT) {
  const r = spawnSync(cmd, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() };
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
        clean: merge && must(["show", "--remerge-diff", "--format=", sha]) === "",
      };
    });
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

function main() {
  const args = process.argv.slice(2);
  const reviewedAt = args.indexOf("--reviewed") + 1;
  const reviewedArg = reviewedAt > 0 ? args[reviewedAt] : undefined;
  const n = args.find((a, i) => i !== reviewedAt && /^\d+$/.test(a));
  if (!n || !args.includes("--reviewed") || !reviewedArg) fail("usage: node scripts/merge-pr.mjs <n> --reviewed <sha> [--dry-run] [--body-file <file>]");

  const view = run("gh", ["pr", "view", n, "--json", "state,isDraft,mergeable,headRefOid,statusCheckRollup"]);
  if (!view.ok) fail(`gh pr view failed: ${view.err}`);
  const pr = JSON.parse(view.out);
  const head = pr.headRefOid;

  const fetch = run("git", ["fetch", "-q", "origin", "main", `pull/${n}/head`]);
  if (!fetch.ok) fail(`git fetch failed: ${fetch.err}`);
  const reviewed = run("git", ["rev-parse", "--verify", "--quiet", `${reviewedArg}^{commit}`]);
  if (!reviewed.ok) fail(`--reviewed ${reviewedArg} names no commit here`);

  const isAncestor = (a, b) => run("git", ["merge-base", "--is-ancestor", a, b]).ok;
  const reviewedInBranch = isAncestor(reviewed.out, head);
  let sinceReview = [];
  try {
    if (reviewedInBranch) sinceReview = commitsSinceReview({ reviewed: reviewed.out, head });
  } catch (e) {
    fail(e.message);
  }

  const reasons = refusals({ pr, containsMain: isAncestor("origin/main", head), reviewedInBranch, sinceReview });
  if (reasons.length) {
    process.stdout.write(`#${n} may not merge:\n${reasons.map((r) => `- ${r}\n`).join("")}`);
    process.exit(1);
  }
  if (args.includes("--dry-run")) {
    process.stdout.write(`#${n} may merge at ${head.slice(0, 7)}\n`);
    process.exit(0);
  }
  const body = args.includes("--body-file") ? ["--body-file", args[args.indexOf("--body-file") + 1]] : [];
  const merge = run("gh", ["pr", "merge", n, "--merge", "--match-head-commit", head, ...body]);
  if (!merge.ok) fail(`gh pr merge failed: ${merge.err}`);
  process.stdout.write(`#${n} merged at ${head.slice(0, 7)}\n`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
