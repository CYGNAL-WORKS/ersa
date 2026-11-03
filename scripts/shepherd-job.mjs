#!/usr/bin/env node
// One headless Claude Code job on one pull request, for the PR shepherd: a
// merge-time review or a fix. The job runs `claude -p` in a worktree detached
// at the pull request's head, made for it alone and removed after, with a
// brief this script writes, a narrow tool list, and a JSON Schema for its
// result, so the caller reads data, not prose.
//
//   node scripts/shepherd-job.mjs review <n>
//   node scripts/shepherd-job.mjs fix <n> --task <file>
//   ... --model <m> --effort <e>   override the job's model and effort
//
// One job per pull request, head and kind: the record of a finished job is
// kept in .scratch/pr-queue/jobs/<n>-<head>-<kind>.json, and asking again for
// the same head prints that record instead of running another. A job that
// returned no result leaves a .failed.json record and may run again. A review with
// findings also writes <n>-<head>-review.task.md, ready to pass to `fix --task`.
// A fix commits in its worktree, and this script pushes the commits only when
// they add to the head it was given.
//
// Exit 0 when the job finished (a clean or findings review, a pushed fix); 1
// when it finished without doing its work (an error, a blocked fix, a push
// that failed or was refused); 2 on a usage or tool error.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const JOBS_DIR = join(ROOT, ".scratch", "pr-queue", "jobs");
const WORK_DIR = join(ROOT, ".scratch", "jobs");
const HOUR = 60 * 60 * 1000;

// Both jobs run the repository's own tests, which is running its code, so the
// tool list states intent rather than a sandbox: Bash for probes and tests, and
// no push, merge or write to GitHub. A deny rule wins over an allow rule and
// applies to each command of a compound one. The script pushes a fix itself.
const TOOLS = ["Read", "Grep", "Glob", "Agent", "Skill", "Edit", "Write", "Bash"];
const NEVER = [
  "PowerShell", "WebFetch", "WebSearch", "Bash(git push:*)",
  "Bash(gh pr merge:*)", "Bash(gh pr comment:*)", "Bash(gh pr review:*)", "Bash(gh pr close:*)", "Bash(gh pr edit:*)", "Bash(gh pr create:*)", "Bash(gh pr ready:*)",
  "Bash(gh issue comment:*)", "Bash(gh issue create:*)", "Bash(gh issue close:*)", "Bash(gh issue edit:*)", "Bash(gh api:*)",
];

const finding = {
  type: "object",
  properties: {
    severity: { enum: ["high", "medium", "low"] },
    location: { type: "string", description: "file:line" },
    defect: { type: "string" },
    scenario: { type: "string", description: "one-line failure scenario" },
  },
  required: ["severity", "location", "defect", "scenario"],
};

export const JOBS = {
  review: {
    model: "fable",
    effort: "high",
    never: [...NEVER, "Bash(git commit:*)"],
    schema: {
      type: "object",
      properties: {
        verdict: { enum: ["clean", "findings"], description: "clean when no confirmed finding of medium severity or above remains" },
        findings: { type: "array", items: finding },
        dismissed: { type: "array", items: { type: "object", properties: { finding: { type: "string" }, evidence: { type: "string" } }, required: ["finding", "evidence"] } },
      },
      required: ["verdict", "findings", "dismissed"],
    },
  },
  fix: {
    model: "opus",
    effort: "medium",
    never: NEVER,
    schema: {
      type: "object",
      properties: {
        outcome: { enum: ["committed", "no change needed", "blocked"] },
        summary: { type: "string", description: "what changed, per task item" },
        inbox: { type: "array", items: { type: "string" }, description: "inbox lines added, verbatim" },
      },
      required: ["outcome", "summary", "inbox"],
    },
  },
};

export function issueOf(body) {
  return /\b(?:fixes|closes|resolves)\s+#(\d+)/i.exec(body ?? "")?.[1];
}

// The paths only a planning session may edit, as scripts/hooks/guard-briefs.mjs
// lists them. A fix to a pull request that changes one needs the planning role.
export function needsPlanning(files) {
  return files.some((f) => /^(docs\/(briefs|adr)\/|docs\/glossary\.md$|claude\.md$|\.claude\/|scripts\/hooks\/)/i.test(f) && !/^docs\/briefs\/inbox\.md$/i.test(f));
}

function about({ number, title, branch, issue, head }) {
  return [
    `Pull request #${number} ("${title}"), branch \`${branch}\`, ${issue ? `which fixes issue #${issue}` : "which names no issue; judge it against its description (`gh pr view " + number + "`)"}.`,
    `This directory is a git worktree detached at the pull request's head, ${head.slice(0, 7)}, made for you alone and deleted when you finish; run tests here.`,
  ].join(" ");
}

export function reviewPrompt(pr) {
  return [
    `You are the merge-time reviewer. ${about(pr)}`,
    "Run the review-pr skill as the orchestrator at merge time, steps 1 to 4, on `git diff origin/main...HEAD`. Restore every file you change for a mutation.",
    "Do not fix, commit, push, or write on GitHub: a fix session acts on what you report.",
    "Return the structured result: every confirmed finding, low ones included, and every dismissal with its evidence. The verdict is clean only when no confirmed finding of medium severity or above remains.",
  ].join("\n\n");
}

export function fixPrompt(pr, task) {
  return [
    `You are the fix session. ${about(pr)}`,
    "Fix only what the task below names, with the smallest change that removes each defect, and a test where one is cheap. Record anything else you find as a line in docs/briefs/INBOX.md. If a task item is wrong, say so in the result instead of changing code for it.",
    "Run `node scripts/check.mjs`, then commit on top of the head you were given; never amend or rebase. Do not push: this job's runner pushes your commits when you finish.",
    `## Task\n\n${task.trim()}`,
  ].join("\n\n");
}

// The task a fix session gets from a review with findings.
export function findingsTask(review, link) {
  const lines = review.findings
    .filter((f) => f.severity !== "low")
    .map((f) => `- **${f.severity}**, ${f.location}: ${f.defect} Failure: ${f.scenario}`);
  const low = review.findings.filter((f) => f.severity === "low").map((f) => `- ${f.location}: ${f.defect}`);
  return [
    `The merge-time review of ${link} confirmed these findings. Fix each one.`,
    lines.join("\n"),
    ...(low.length ? [`Fix these low ones only if trivial; list the rest in your summary:\n\n${low.join("\n")}`] : []),
  ].join("\n\n");
}

export function claudeArgs(job, prompt) {
  return [
    "-p", prompt,
    "--model", job.model,
    "--effort", job.effort,
    "--output-format", "json",
    "--json-schema", JSON.stringify(job.schema),
    "--permission-mode", "acceptEdits",
    "--allowedTools", ...TOOLS,
    "--disallowedTools", ...job.never,
  ];
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, ...opts });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() || (r.error?.message ?? "") };
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

// Pushes a fix's commits, only when they add to the head the job was given:
// no force, so a branch that moved meanwhile refuses the push.
function pushFix(dir, pr) {
  const head = run("git", ["rev-parse", "HEAD"], { cwd: dir }).out;
  const uncommitted = run("git", ["status", "--porcelain"], { cwd: dir }).out.split("\n").filter(Boolean).length;
  if (head === pr.head) return { pushedHead: null, uncommitted };
  if (!run("git", ["merge-base", "--is-ancestor", pr.head, head], { cwd: dir }).ok)
    return { pushedHead: null, uncommitted, error: "rewrote the branch's history; nothing pushed" };
  const push = run("git", ["push", "origin", `HEAD:${pr.branch}`], { cwd: dir });
  return push.ok ? { pushedHead: head, uncommitted } : { pushedHead: null, uncommitted, error: `push failed: ${push.err.slice(0, 300)}` };
}

function main() {
  const [kind, n, ...rest] = process.argv.slice(2);
  const flag = (name) => (rest.includes(name) ? rest[rest.indexOf(name) + 1] ?? "" : undefined);
  const taskFile = flag("--task");
  if (!JOBS[kind] || !/^\d+$/.test(n ?? "") || (kind === "fix" && !taskFile) || [flag("--model"), flag("--effort")].includes(""))
    fail("usage: node scripts/shepherd-job.mjs review <n> | fix <n> --task <file>  [--model <m>] [--effort <e>]");
  const job = { ...JOBS[kind], model: flag("--model") ?? JOBS[kind].model, effort: flag("--effort") ?? JOBS[kind].effort };

  const view = run("gh", ["pr", "view", n, "--json", "state,title,headRefName,headRefOid,body,url"]);
  if (!view.ok) fail(`gh pr view failed: ${view.err}`);
  const v = JSON.parse(view.out);
  if (v.state !== "OPEN") fail(`#${n} is ${v.state.toLowerCase()}`);
  const pr = { number: n, title: v.title, branch: v.headRefName, head: v.headRefOid, issue: issueOf(v.body) };

  const id = `${n}-${pr.head.slice(0, 7)}-${kind}`;
  const recordFile = join(JOBS_DIR, `${id}.json`);
  if (existsSync(recordFile)) {
    process.stdout.write(`#${n}: a ${kind} of ${pr.head.slice(0, 7)} already ran; its record is ${recordFile}\n${readFileSync(recordFile, "utf8")}\n`);
    process.exit(0);
  }

  const fetch = run("git", ["fetch", "-q", "origin", "main", `pull/${n}/head`]);
  if (!fetch.ok) fail(`git fetch failed: ${fetch.err}`);
  const files = run("git", ["diff", "--name-only", `origin/main...${pr.head}`]).out.split("\n").filter(Boolean);
  const dir = join(WORK_DIR, id);
  mkdirSync(WORK_DIR, { recursive: true });
  const add = run("git", ["worktree", "add", "--detach", dir, pr.head]);
  if (!add.ok) fail(`git worktree add failed: ${add.err}`);

  const prompt = kind === "review" ? reviewPrompt(pr) : fixPrompt(pr, readFileSync(taskFile, "utf8"));
  const started = Date.now();
  const env = {
    ...process.env,
    // A fix edits the pull request's own files, planning ones included.
    ERSA_ROLE: kind === "fix" && needsPlanning(files) ? "planning" : "implementing",
    // One build directory for every job, so each worktree does not rebuild Core.
    CARGO_TARGET_DIR: join(WORK_DIR, "target"),
  };
  const claude = run("claude", claudeArgs(job, prompt), { cwd: dir, env, timeout: 2 * HOUR, stdio: ["ignore", "pipe", "pipe"] });
  let result = {};
  try {
    result = JSON.parse(claude.out);
  } catch {
    result = { is_error: true, subtype: `no JSON result: ${claude.err.slice(0, 300)}` };
  }
  // Only a fix that finished and says it committed gets its commits pushed.
  const pushed = kind === "fix" && !result.is_error && result.structured_output?.outcome === "committed" ? pushFix(dir, pr) : {};
  run("git", ["worktree", "remove", "--force", dir]);

  const record = {
    kind, pr: Number(n), head: pr.head, branch: pr.branch, issue: pr.issue ?? null,
    startedAt: new Date(started).toISOString(), minutes: Math.round((Date.now() - started) / 60000),
    model: job.model, effort: job.effort, role: env.ERSA_ROLE,
    error: result.is_error ? result.subtype ?? "error" : null,
    costUsd: result.total_cost_usd ?? null, turns: result.num_turns ?? null, session: result.session_id ?? null,
    denied: (result.permission_denials ?? []).map((d) => `${d.tool_name}: ${d.tool_input?.command ?? ""}`.trim()),
    output: result.structured_output ?? null,
  };
  if (kind === "fix") {
    Object.assign(record, pushed, { error: record.error ?? pushed.error ?? null });
    if (!record.error && record.output?.outcome === "committed" && !record.pushedHead) record.error = "reported commits, but made none";
  }

  // A job that returned no result did no work, so it does not use up its head:
  // its record goes aside and the next ask runs it again.
  mkdirSync(JOBS_DIR, { recursive: true });
  writeFileSync(record.output ? recordFile : join(JOBS_DIR, `${id}.${started}.failed.json`), JSON.stringify(record, null, 2));
  if (kind === "review" && record.output?.verdict === "findings")
    writeFileSync(join(JOBS_DIR, `${id}.task.md`), findingsTask(record.output, v.url));
  process.stdout.write(`${JSON.stringify(record, null, 2)}\n`);

  const worked = !record.error && record.output && (kind === "review" || record.output.outcome !== "blocked");
  process.exit(worked ? 0 : 1);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
