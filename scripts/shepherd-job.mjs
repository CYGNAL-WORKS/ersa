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
// returned no result or ended in an error (a push that failed, say) leaves a
// .failed.json record, naming any commits it made, and may run again. A review with
// findings also writes <n>-<head>-review.task.md, ready to pass to `fix --task`.
// A fix commits in its worktree, and this script pushes the commits only when
// they add to the head it was given.
//
// Exit 0 when the job finished (a clean or findings review, a pushed fix); 1
// when it finished without doing its work (an error, a blocked fix, a push
// that failed or was refused); 2 on a usage or tool error.
//
// A job runs the pull request's code (its tests) with the user's git and gh
// credentials, so it runs only on pull requests from this repository's own
// branches, never from a fork. The pull request's own hooks and MCP servers
// are not loaded; its CLAUDE.md and skills are, as instructions.

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const JOBS_DIR = join(ROOT, ".scratch", "pr-queue", "jobs");
const WORK_DIR = join(ROOT, ".scratch", "jobs");
const HOUR = 60 * 60 * 1000;

// Both jobs run the repository's own tests, which is running its code, so the
// tool list states intent rather than a sandbox: Bash for probes and tests, and
// the plain spellings of a push, a merge or a write to GitHub refused. The deny
// rules are prefixes, so `git -C . push`, `bash -c` or an alias gets past them,
// and so does any test the pull request adds: they stop a job's habits, not a
// hostile pull request. That is why jobs run only on this repository's own
// branches. A deny rule wins over an allow rule and applies to each command of
// a compound one. The script pushes a fix itself.
export const TOOLS = ["Read", "Grep", "Glob", "Agent", "Skill", "Edit", "Write", "Bash"];
export const NEVER = [
  "PowerShell", "WebFetch", "WebSearch", "Bash(curl:*)", "Bash(git push:*)",
  "Bash(gh pr merge:*)", "Bash(gh pr comment:*)", "Bash(gh pr review:*)", "Bash(gh pr close:*)", "Bash(gh pr reopen:*)", "Bash(gh pr edit:*)", "Bash(gh pr create:*)", "Bash(gh pr ready:*)",
  "Bash(gh issue comment:*)", "Bash(gh issue create:*)", "Bash(gh issue close:*)", "Bash(gh issue edit:*)", "Bash(gh api:*)",
  "Bash(gh workflow:*)", "Bash(gh run rerun:*)", "Bash(gh run cancel:*)", "Bash(gh release:*)", "Bash(gh repo:*)", "Bash(gh label:*)",
  "Bash(gh secret:*)", "Bash(gh variable:*)", "Bash(gh gist:*)", "Bash(gh auth:*)",
];
// The pull request's own hooks would run outside the deny rules, so the job
// loads none; its .mcp.json servers likewise.
const SETTINGS = JSON.stringify({ disableAllHooks: true });

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
    model: "opus",
    effort: "medium",
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
  return files.some((file) => {
    const f = file.replace(/\\/g, "/").toLowerCase();
    const guarded =
      /(^|\/)docs\/(briefs|adr)\//.test(f) || /(^|\/)docs\/glossary\.md$/.test(f) || /(^|\/)claude\.md$/.test(f) ||
      /(^|\/)\.claude\/(skills\/|settings\.json$)/.test(f) || /(^|\/)scripts\/hooks\//.test(f);
    const open = /(^|\/)docs\/briefs\/inbox\.md$/.test(f) || /(^|\/)docs\/briefs\/protocol\/fixtures\//.test(f);
    return guarded && !open;
  });
}

// The environment a job runs in. The PR's hooks are off, so the planning-path
// guard does not run in the job; pushFix applies the same rule to its commits.
export function jobEnv(kind, files, base, workDir) {
  return {
    ...base,
    // A fix edits the pull request's own files, planning ones included.
    ERSA_ROLE: kind === "fix" && needsPlanning(files) ? "planning" : "implementing",
    // One build directory for every job, so each worktree does not rebuild Core.
    CARGO_TARGET_DIR: join(workDir, "target"),
  };
}

// Only a job that returned a result without an error uses up its head; any
// other record goes aside, so the next ask runs the job again.
export function recordName(id, record, started) {
  return record.output && !record.error ? `${id}.json` : `${id}.${started}.failed.json`;
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
    "--settings", SETTINGS,
    "--strict-mcp-config",
    "--allowedTools", ...TOOLS,
    "--disallowedTools", ...job.never,
  ];
}

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, ...opts });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() || (r.error?.message ?? "") };
}

function killTree(pid) {
  if (process.platform === "win32") spawnSync("taskkill", ["/T", "/F", "/PID", String(pid)], { stdio: "ignore" });
  else {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {}
  }
}

// Runs a command and, at the timeout, kills it with every process it started
// (tests, builds, subagents), so none outlives the worktree it runs in.
export function runTree(cmd, args, opts, timeoutMs) {
  return new Promise((done) => {
    let out = "", err = "", timedOut = false, settled = false;
    const finish = (status) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      const why = timedOut ? `timed out after ${Math.round(timeoutMs / 60000)} minutes` : "";
      done({ ok: status === 0 && !timedOut, out: out.trim(), err: [why, err.trim()].filter(Boolean).join("; ") });
    };
    const child = spawn(cmd, args, { ...opts, stdio: ["ignore", "pipe", "pipe"], detached: process.platform !== "win32" });
    child.stdout.setEncoding("utf8").on("data", (d) => (out += d));
    child.stderr.setEncoding("utf8").on("data", (d) => (err += d));
    const timer = setTimeout(() => {
      timedOut = true;
      killTree(child.pid);
    }, timeoutMs);
    child.on("error", (e) => {
      err += e.message;
      finish(null);
    });
    child.on("close", finish);
  });
}

// A digest of the git config and hooks a worktree shares with the checkout it
// came from. A job that changes either could run code at the script's push, or
// redirect it, and in every later git command in the user's checkout.
export function gitFingerprint(commonDir) {
  const hash = createHash("sha256");
  const add = (path) => {
    if (!existsSync(path)) return hash.update(`${path}:none\n`);
    if (statSync(path).isDirectory()) for (const entry of readdirSync(path).sort()) add(join(path, entry));
    else hash.update(`${path}\n`).update(readFileSync(path));
  };
  add(join(commonDir, "config"));
  add(join(commonDir, "hooks"));
  return hash.digest("hex");
}

// Removes a job's worktree, including one left by a crashed or killed run.
function clearWorktree(dir) {
  run("git", ["worktree", "remove", "--force", dir]);
  run("git", ["worktree", "prune"]);
  if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
}

function writeAtomic(file, text) {
  writeFileSync(`${file}.tmp`, text);
  renameSync(`${file}.tmp`, file);
}

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(2);
}

// Pushes a fix's commits, only when they add to the head the job was given:
// no force, so a branch that moved meanwhile refuses the push, and no hooks.
// An implementing fix that touched a planning path is refused, as the guard
// hook would have refused the edit. commitHead names the commits either way.
export function pushFix(dir, pr, role) {
  const head = run("git", ["rev-parse", "HEAD"], { cwd: dir }).out;
  const uncommitted = run("git", ["status", "--porcelain"], { cwd: dir }).out.split("\n").filter(Boolean).length;
  const base = { commitHead: head, pushedHead: null, uncommitted };
  if (head === pr.head) return base;
  if (!run("git", ["merge-base", "--is-ancestor", pr.head, head], { cwd: dir }).ok)
    return { ...base, error: "rewrote the branch's history; nothing pushed" };
  const changed = run("git", ["diff", "--name-only", pr.head, head], { cwd: dir }).out.split("\n").filter(Boolean);
  if (role !== "planning" && needsPlanning(changed))
    return { ...base, error: "changed a planning path as implementing; nothing pushed" };
  const push = run("git", ["push", "--no-verify", "origin", `HEAD:${pr.branch}`], { cwd: dir });
  return push.ok ? { ...base, pushedHead: head } : { ...base, error: `push failed: ${push.err.slice(0, 300)}` };
}

async function main() {
  const [kind, n, ...rest] = process.argv.slice(2);
  const flag = (name) => {
    if (!rest.includes(name)) return undefined;
    const value = rest[rest.indexOf(name) + 1] ?? "";
    return value.startsWith("--") ? "" : value;
  };
  const taskFile = flag("--task");
  if (!Object.hasOwn(JOBS, kind ?? "") || !/^\d+$/.test(n ?? "") || (kind === "fix" && !taskFile) || [flag("--model"), flag("--effort")].includes(""))
    fail("usage: node scripts/shepherd-job.mjs review <n> | fix <n> --task <file>  [--model <m>] [--effort <e>]");
  const job = { ...JOBS[kind], model: flag("--model") ?? JOBS[kind].model, effort: flag("--effort") ?? JOBS[kind].effort };
  let task = "";
  if (kind === "fix") {
    try {
      task = readFileSync(taskFile, "utf8");
    } catch (e) {
      fail(`cannot read the task file: ${e.message}`);
    }
  }

  const view = run("gh", ["pr", "view", n, "--json", "state,title,headRefName,headRefOid,body,url,isCrossRepository"]);
  if (!view.ok) fail(`gh pr view failed: ${view.err}`);
  const v = JSON.parse(view.out);
  if (v.state !== "OPEN") fail(`#${n} is ${v.state.toLowerCase()}`);
  if (v.isCrossRepository !== false) fail(`#${n} comes from a fork; shepherd jobs run only on this repository's own branches`);
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
  clearWorktree(dir);
  const add = run("git", ["worktree", "add", "--detach", dir, pr.head]);
  if (!add.ok) fail(`git worktree add failed: ${add.err}`);

  const prompt = kind === "review" ? reviewPrompt(pr) : fixPrompt(pr, task);
  const started = Date.now();
  const env = jobEnv(kind, files, process.env, WORK_DIR);
  const commonDir = run("git", ["rev-parse", "--git-common-dir"]).out;
  const gitDir = isAbsolute(commonDir) ? commonDir : join(ROOT, commonDir);
  const before = gitFingerprint(gitDir);
  const claude = await runTree("claude", claudeArgs(job, prompt), { cwd: dir, env }, 2 * HOUR);
  let result = {};
  try {
    result = JSON.parse(claude.out);
  } catch {
    result = { is_error: true, subtype: `no JSON result: ${claude.err.slice(0, 300)}` };
  }
  const tampered = gitFingerprint(gitDir) !== before;
  // Only a fix that finished and says it committed gets its commits pushed.
  const pushed =
    kind === "fix" && !tampered && !result.is_error && result.structured_output?.outcome === "committed" ? pushFix(dir, pr, env.ERSA_ROLE) : {};
  clearWorktree(dir);

  const record = {
    kind, pr: Number(n), head: pr.head, branch: pr.branch, issue: pr.issue ?? null,
    startedAt: new Date(started).toISOString(), minutes: Math.round((Date.now() - started) / 60000),
    model: job.model, effort: job.effort, role: env.ERSA_ROLE,
    error: result.is_error ? result.subtype ?? "error" : null,
    costUsd: result.total_cost_usd ?? null, turns: result.num_turns ?? null, session: result.session_id ?? null,
    denied: (result.permission_denials ?? []).map((d) => `${d.tool_name}: ${d.tool_input?.command ?? ""}`.trim()),
    output: result.structured_output ?? null,
  };
  if (tampered) record.error = "the job changed .git/config or .git/hooks, which the user's checkout shares; nothing pushed. Inspect both.";
  if (kind === "fix") {
    Object.assign(record, pushed, { error: record.error ?? pushed.error ?? null });
    if (!record.error && record.output?.outcome === "committed" && !record.pushedHead) record.error = "reported commits, but made none";
  }

  mkdirSync(JOBS_DIR, { recursive: true });
  writeAtomic(join(JOBS_DIR, recordName(id, record, started)), JSON.stringify(record, null, 2));
  if (kind === "review" && !record.error && record.output?.verdict === "findings")
    writeAtomic(join(JOBS_DIR, `${id}.task.md`), findingsTask(record.output, v.url));
  process.stdout.write(`${JSON.stringify(record, null, 2)}\n`);

  const worked = !record.error && record.output && (kind === "review" || record.output.outcome !== "blocked");
  process.exit(worked ? 0 : 1);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
