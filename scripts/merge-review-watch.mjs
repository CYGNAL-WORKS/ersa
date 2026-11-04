#!/usr/bin/env node
// The merge review poller (ADR 0023): on the owner's machine, with the owner's
// gh and Claude Code logins, it finds every open pull request from this
// repository whose CI passed on a head that carries no merge review yet, and
// runs scripts/merge-review.mjs on each, one at a time.
//
//   node scripts/merge-review-watch.mjs [--once] [--checkout <dir>]
//
// Without --once it ticks every five minutes until stopped; --once runs one
// tick, which is what the scheduled task runs. A tick lists the open pull
// requests, skips drafts and forks, and reads two things off each head:
// whether its latest CI run passed (as the gate does), and whether it carries
// a "Merge review" commit status, which the review script sets as soon as it
// starts on a head. A head with the status is reviewed, or being reviewed; a
// pending status older than an hour is a run that died with the machine, and
// the head is reviewed again. A head without the status that GitHub would
// merge on its own (auto-merge armed by a review of an earlier head, which a
// push by the owner's account does not disarm) gets auto-merge disabled first,
// so nothing unreviewed enters the queue. A lock file in .scratch/ holds the
// pid of the running tick, so two ticks never overlap; a lock whose process is
// gone is taken over.
//
// Each review runs in its own checkout, a detached worktree of this repository
// at <dir> (by default a sibling folder named after this one with "-review"
// appended), reset to the head with the review script and the gate taken from
// main, as the retired workflow did; the review script takes the hooks and the
// instruction files from main itself.
//
// The scheduled task, registered once from a PowerShell prompt in the
// repository, with node on the task's PATH (the user's PATH, which is where
// claude and gh must be too). Runs only while the owner is logged on, since
// both logins are theirs:
//
//   $run = New-ScheduledTaskAction -Execute "node" -Argument "scripts\merge-review-watch.mjs --once" -WorkingDirectory (Get-Location)
//   $every5 = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5)
//   $limits = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Hours 1)
//   Register-ScheduledTask -TaskName "Ersa merge review" -Action $run -Trigger $every5 -Settings $limits
//
// Exit 0 after a tick, or at once when another tick holds the lock; 2 on a
// usage or tool error.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { MERGE_REVIEW, ciRefusals, normalizeChecks } from "./merge-pr.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const LOCK = join(ROOT, ".scratch", "merge-review-watch.lock");
const CHECKOUT = join(ROOT, "..", `${basename(ROOT)}-review`);
export const INTERVAL_MS = 5 * 60 * 1000;
// Longer than a whole review run (two minutes a tool call, 38 and 4 for claude).
export const STALE_MS = 60 * 60 * 1000;
const FIELDS = "number,isDraft,isCrossRepository,headRefOid,autoMergeRequest,statusCheckRollup";

// Which pull requests a tick acts on: `review` the heads to review, `disarm`
// the numbers whose unreviewed head has auto-merge on. Pure, so a test can
// drive every case with gh's JSON alone.
export function select(prs, now = Date.now()) {
  const review = [];
  const disarm = [];
  for (const pr of prs) {
    if (pr.isDraft || pr.isCrossRepository) continue;
    const mark = normalizeChecks(pr.statusCheckRollup).find((c) => c.name === MERGE_REVIEW);
    const died = mark?.status === "PENDING" && now - Date.parse(mark.startedAt ?? "") > STALE_MS;
    const reviewed = Boolean(mark) && !died;
    if (!reviewed && pr.autoMergeRequest) disarm.push(pr.number);
    if (!reviewed && !ciRefusals(pr.statusCheckRollup).length) review.push({ number: pr.number, head: pr.headRefOid });
  }
  return { review, disarm };
}

// Takes the lock for this process and returns what releases it, or null when
// a live process holds it. A lock whose pid is gone is a tick that died and
// is taken over; one that names no pid is left for a person to look at.
export function lock(path, { alive = isAlive, pid = process.pid } = {}) {
  mkdirSync(dirname(path), { recursive: true });
  for (;;) {
    try {
      writeFileSync(path, String(pid), { flag: "wx" });
      return () => rmSync(path, { force: true });
    } catch (e) {
      if (e.code !== "EEXIST") throw e;
    }
    let holder;
    try {
      holder = Number(readFileSync(path, "utf8"));
    } catch (e) {
      if (e.code === "ENOENT") continue;
      throw e;
    }
    if (!Number.isInteger(holder) || holder <= 0 || alive(holder)) return null;
    rmSync(path, { force: true });
  }
}

// Signal 0 tests for the process without touching it; a live process the
// signal may not reach (EPERM) is still a live process.
function isAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === "EPERM";
  }
}

// Git and gh stall on a prompt or a dead network; the review itself has its
// own limits and runs with the console, so its output is seen live.
function run(cmd, args, cwd = ROOT, { live = false } = {}) {
  const r = spawnSync(cmd, args, {
    cwd,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: live ? undefined : 2 * 60 * 1000,
    stdio: live ? "inherit" : "pipe",
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0", GH_PROMPT_DISABLED: "1" },
  });
  return { ok: r.status === 0, status: r.status, out: (r.stdout ?? "").trim(), err: [(r.stderr ?? "").trim(), r.error?.message].filter(Boolean).join("; ") };
}

const pause = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
const stamp = () => new Date().toISOString().replace(/\.\d+Z$/, "Z");

class ToolError extends Error {}
const fail = (m) => {
  throw new ToolError(m);
};

// Puts the review checkout at `head` with main's review logic in it. The
// first tick makes the worktree; each tick fetches main and the pull
// request's head into the shared object store, then resets the tree.
function prepare(run, dir, n, head) {
  const must = (args, cwd) => {
    const r = run("git", args, cwd);
    if (!r.ok) fail(`git ${args.slice(0, 2).join(" ")} failed: ${r.err}`);
  };
  if (!existsSync(join(dir, ".git"))) must(["worktree", "add", "--detach", dir, "origin/main"], ROOT);
  must(["fetch", "-q", "origin", "main", `pull/${n}/head`], dir);
  must(["reset", "-q", "--hard", head], dir);
  must(["clean", "-q", "-fd"], dir);
  must(["checkout", "-q", "origin/main", "--", "scripts/merge-review.mjs", "scripts/merge-pr.mjs"], dir);
}

// One pass. A pull request whose checkout cannot be prepared is reported and
// skipped, and the next tick tries it again; the review's own exit code is
// reported and never stops the tick.
function tick(run, dir, log, now) {
  const list = run("gh", ["pr", "list", "--state", "open", "--limit", "100", "--json", FIELDS]);
  if (!list.ok) fail(`gh pr list failed: ${list.err}`);
  const { review, disarm } = select(JSON.parse(list.out), now());
  for (const n of disarm) {
    const r = run("gh", ["pr", "merge", String(n), "--disable-auto"]);
    log(`#${n}: auto-merge was on for an unreviewed head; ${r.ok ? "disabled it" : `could not disable it: ${r.err}`}`);
  }
  for (const { number: n, head } of review) {
    log(`#${n} at ${head.slice(0, 7)}: reviewing`);
    try {
      prepare(run, dir, n, head);
    } catch (e) {
      log(`#${n} at ${head.slice(0, 7)}: ${e.message}`);
      continue;
    }
    const r = run("node", ["scripts/merge-review.mjs", String(n), "--head", head], dir, { live: true });
    log(`#${n} at ${head.slice(0, 7)}: the review exited ${r.status ?? r.err}`);
  }
  if (!review.length) log("nothing to review");
}

const USAGE = "usage: node scripts/merge-review-watch.mjs [--once] [--checkout <dir>]";

// Runs the poller for argv. `run` runs one command, `sleep` waits between
// ticks, `now` tells the time and `log` takes each line, so a test can stand
// in for every tool and the clock.
export function main(argv, { run: r = run, sleep = pause, now = Date.now, log = (l) => process.stdout.write(`${stamp()} ${l}\n`), lockPath = LOCK, alive } = {}) {
  let once = false;
  let dir = CHECKOUT;
  try {
    for (let i = 0; i < argv.length; i++) {
      if (argv[i] === "--once") once = true;
      else if (argv[i] === "--checkout" && i + 1 < argv.length) dir = resolve(argv[++i]);
      else fail(`unrecognized argument ${argv[i]}\n${USAGE}`);
    }
    const release = lock(lockPath, alive ? { alive } : {});
    if (!release) return { code: 0, stdout: `another tick holds ${lockPath}; nothing to do\n` };
    try {
      for (;;) {
        tick(r, dir, log, now);
        if (once) return { code: 0 };
        sleep(INTERVAL_MS);
      }
    } finally {
      release();
    }
  } catch (e) {
    return { code: 2, stderr: `${e instanceof ToolError ? e.message : e.stack}\n` };
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { code, stdout, stderr } = main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
