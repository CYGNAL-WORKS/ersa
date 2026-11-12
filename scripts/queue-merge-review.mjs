#!/usr/bin/env node
// The "Merge review" check on a merge queue entry. Docket sets the status of
// that name on pull request heads only, and the queue tests a temporary merge
// commit, so a ruleset requiring the status would wait on the queue's commit
// until it timed out. This check gives that commit the head's answer.
//
//   QUEUE_REF=<merge_group.head_ref> GITHUB_REPOSITORY=<owner/name> node scripts/queue-merge-review.mjs
//
// It passes only when the pull request's current head carries a successful
// "Merge review" status. Otherwise it fails and says why.

import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const CONTEXT = "Merge review";

const WHY = {
  pending: "its merge review is still running",
  failure: "its merge review blocks it",
  error: "its merge review could not finish",
};

// A stalled gh call ends after a minute, well inside the job's own limit.
function run(cmd, args) {
  const r = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 16 * 1024 * 1024, timeout: 60 * 1000 });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: [(r.stderr ?? "").trim(), r.error?.message].filter(Boolean).join("; ") };
}

// The queue's branch is refs/heads/gh-readonly-queue/<base>/pr-<n>-<base sha>.
export function pullRequestOf(ref) {
  return /\/pr-(\d+)-[0-9a-f]+$/.exec(ref ?? "")?.[1] ?? null;
}

// GitHub lists a commit's statuses newest first, so the first of a context is
// its current state.
export function reviewState(statuses) {
  return statuses.find((s) => s?.context === CONTEXT)?.state ?? null;
}

function api(r, path) {
  const x = r("gh", ["api", path]);
  if (!x.ok) return { failed: x.err.replace(/\s*\n\s*/g, " ").slice(0, 300) };
  try {
    return { value: JSON.parse(x.out) };
  } catch {
    return { failed: `${path} answered no JSON` };
  }
}

export function main({ ref, repo }, { run: r = run } = {}) {
  const fail = (why) => ({ code: 1, stderr: `${why}\n` });
  const number = pullRequestOf(ref);
  if (!number) return fail(`Could not tell which pull request the queue entry ${ref || "(no ref)"} is for.`);
  const pull = api(r, `repos/${repo}/pulls/${number}`);
  if (pull.failed || typeof pull.value?.head?.sha !== "string") return fail(`Could not read pull request #${number}: ${pull.failed ?? "it names no head"}.`);
  const head = pull.value.head.sha;
  const statuses = api(r, `repos/${repo}/commits/${head}/statuses?per_page=100`);
  if (statuses.failed || !Array.isArray(statuses.value)) return fail(`Could not read the statuses of ${head.slice(0, 7)}: ${statuses.failed ?? "not a list"}.`);
  const state = reviewState(statuses.value);
  const at = `Pull request #${number} at ${head.slice(0, 7)}`;
  if (state === "success") return { code: 0, stdout: `${at} passed its merge review.\n` };
  return fail(`${at} can't merge: ${WHY[state] ?? "it has no merge review"}.`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const { code, stdout, stderr } = main({ ref: process.env.QUEUE_REF, repo: process.env.GITHUB_REPOSITORY });
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
