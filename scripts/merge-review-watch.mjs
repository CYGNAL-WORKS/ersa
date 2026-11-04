#!/usr/bin/env node
// The merge review poller (ADR 0023): on the owner's machine, with the owner's
// gh and Claude Code logins, it finds every open pull request from this
// repository whose CI passed on a head that carries no merge review yet, and
// runs scripts/merge-review.mjs on one of them per tick.
//
//   node scripts/merge-review-watch.mjs [--once] [--checkout <dir>]
//
// Without --once it ticks every five minutes until stopped, for a terminal;
// --once runs one tick, which is what the scheduled task runs. A tick lists the
// open pull requests, skips drafts and forks, and reads two things off each
// head: whether its latest CI run passed (as the gate does), and whether it
// carries a "Merge review" commit status, which the review script sets as soon
// as it starts on a head. A head with the status is reviewed, being reviewed,
// or clean and waiting for the gate; a pending status older than an hour is
// one of the last two gone stale (a run that died with the machine, a gate
// that may pass now), and the head is taken up again. One head per tick, so a
// tick ends within the hour the task allows it. A head without the status that
// GitHub would merge on its own (auto-merge armed by a review of an earlier
// head, which a push by the owner's account does not disarm), or any head of a
// pull request labeled `hold`, gets auto-merge disabled first, so nothing
// unreviewed or held enters the queue. A lock file in .scratch/ holds the pid
// of the running tick, so two ticks never overlap; a lock whose process is gone
// or that is older than an hour is taken over.
//
// Each review runs in its own checkout, a detached worktree of this repository
// at <dir> (by default a sibling folder named after this one with "-review"
// appended), reset to the head with the review script and the gate taken from
// main, as the retired workflow did, and every file under a .claude folder
// removed, ignored ones included; the review script takes the hooks and the
// instruction files from main itself. The review runs the pull request's own
// tests as the owner, with the owner's gh login in the environment: that is
// what "never a fork" protects, and why the repository's default workflow
// token stays read-only, so a pull request's own workflow cannot post the
// status or a review.
//
// The scheduled task, registered once from a PowerShell prompt in the
// repository, with node, gh and claude on the user's PATH. It runs only while
// the owner is logged on, since both logins are theirs, and appends the
// poller's lines and each review's output to .scratch\merge-review-watch.log:
//
//   $run = New-ScheduledTaskAction -Execute "cmd" -Argument "/c node scripts\merge-review-watch.mjs --once >> .scratch\merge-review-watch.log 2>&1" -WorkingDirectory (Get-Location)
//   $every5 = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5)
//   $limits = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Hours 1)
//   Register-ScheduledTask -TaskName "Ersa merge review" -Action $run -Trigger $every5 -Settings $limits
//
// Exit 0 after a tick, or at once when another tick holds the lock; 2 on a
// usage or tool error.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { MERGE_REVIEW, ciRefusals, normalizeChecks } from "./merge-pr.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const LOCK = join(ROOT, ".scratch", "merge-review-watch.lock");
const CHECKOUT = join(ROOT, "..", `${basename(ROOT)}-review`);
export const INTERVAL_MS = 5 * 60 * 1000;
// Longer than a whole review run (two minutes a tool call, 38 and 4 for
// claude): a pending status or a lock older than this belongs to a run that
// is over.
export const STALE_MS = 60 * 60 * 1000;
const FIELDS = "number,isDraft,isCrossRepository,headRefOid,labels,autoMergeRequest,statusCheckRollup";

// Which pull requests a tick acts on: `review` the heads to review, in list
// order, `disarm` the numbers whose auto-merge must go. Pure, so a test can
// drive every case with gh's JSON alone.
export function select(prs, now = Date.now()) {
  const review = [];
  const disarm = [];
  for (const pr of prs) {
    if (pr.isDraft || pr.isCrossRepository) continue;
    const mark = normalizeChecks(pr.statusCheckRollup).find((c) => c.name === MERGE_REVIEW);
    const stale = mark?.status === "PENDING" && now - Date.parse(mark.startedAt ?? "") > STALE_MS;
    const reviewed = Boolean(mark) && !stale;
    const held = (pr.labels ?? []).some((l) => l.name === "hold");
    if (pr.autoMergeRequest && (!reviewed || held)) disarm.push(pr.number);
    if (!reviewed && !ciRefusals(pr.statusCheckRollup).length) review.push({ number: pr.number, head: pr.headRefOid });
  }
  return { review, disarm };
}

// Takes the lock for this process and returns what releases it, or null when
// a live tick holds it. A lock is a tick that died when its pid is gone or
// the file is older than STALE_MS (a pid Windows gave to another process reads
// as live, so age settles it); it is taken over by renaming it away, which
// only one of two ticks reading the same dead lock can do. A lock that names
// no pid is left for a person to look at.
export function lock(path, { alive = isAlive, pid = process.pid, now = Date.now } = {}) {
  mkdirSync(dirname(path), { recursive: true });
  for (;;) {
    try {
      writeFileSync(path, String(pid), { flag: "wx" });
      return () => rmSync(path, { force: true });
    } catch (e) {
      if (e.code !== "EEXIST") throw e;
    }
    let holder;
    let age;
    try {
      holder = Number(readFileSync(path, "utf8"));
      age = now() - statSync(path).mtimeMs;
    } catch (e) {
      if (e.code === "ENOENT") continue;
      throw e;
    }
    if (!Number.isInteger(holder) || holder <= 0) return null;
    if (alive(holder) && age < STALE_MS) return null;
    const gone = `${path}.${pid}.stale`;
    try {
      renameSync(path, gone);
    } catch (e) {
      if (e.code === "ENOENT") continue;
      throw e;
    }
    rmSync(gone, { force: true });
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
// first tick makes the worktree, after pruning one whose folder is gone;
// each tick fetches main and the pull request's head into the shared object
// store, resets the tree, and removes what the last review left under any
// .claude folder, ignored files included (a settings.local.json the tests
// wrote would be instructions that are not main's).
function prepare(run, dir, n, head) {
  const must = (args, cwd) => {
    const r = run("git", args, cwd);
    if (!r.ok) fail(`git ${args.slice(0, 2).join(" ")} failed: ${r.err}`);
  };
  if (!existsSync(join(dir, ".git"))) {
    must(["worktree", "prune"], ROOT);
    must(["worktree", "add", "--detach", dir, "origin/main"], ROOT);
  }
  must(["fetch", "-q", "origin", "main", `pull/${n}/head`], dir);
  must(["reset", "-q", "--hard", head], dir);
  must(["clean", "-q", "-fd"], dir);
  must(["clean", "-q", "-fdx", "--", ":(glob)**/.claude/**"], dir);
  must(["checkout", "-q", "origin/main", "--", "scripts/merge-review.mjs", "scripts/merge-pr.mjs"], dir);
}

// One pass: disarm what must not merge, then review the first head due. A
// checkout that cannot be prepared is reported, and the next tick tries
// again; the review's own exit code is reported, whatever it is.
function tick(run, dir, log, now) {
  const list = run("gh", ["pr", "list", "--state", "open", "--limit", "100", "--json", FIELDS]);
  if (!list.ok) fail(`gh pr list failed: ${list.err}`);
  const { review, disarm } = select(JSON.parse(list.out), now());
  for (const n of disarm) {
    const r = run("gh", ["pr", "merge", String(n), "--disable-auto"]);
    log(`#${n}: auto-merge was on for an unreviewed or held head; ${r.ok ? "disabled it" : `could not disable it: ${r.err}`}`);
  }
  if (!review.length) return log("nothing to review");
  const [{ number: n, head }, ...later] = review;
  log(`#${n} at ${head.slice(0, 7)}: reviewing${later.length ? `; ${later.map((p) => `#${p.number}`).join(", ")} next` : ""}`);
  try {
    prepare(run, dir, n, head);
  } catch (e) {
    return log(`#${n} at ${head.slice(0, 7)}: ${e.message}`);
  }
  const r = run("node", ["scripts/merge-review.mjs", String(n), "--head", head], dir, { live: true });
  log(`#${n} at ${head.slice(0, 7)}: the review exited ${r.status ?? r.err}`);
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
    const release = lock(lockPath, { now, ...(alive ? { alive } : {}) });
    if (!release) return { code: 0, stdout: `another tick holds ${lockPath}; nothing to do\n` };
    try {
      for (;;) {
        // In a terminal a tool that is down for one tick is not the end of
        // the loop; a single tick, the task's, says so and ends.
        try {
          tick(r, dir, log, now);
        } catch (e) {
          if (once || !(e instanceof ToolError)) throw e;
          log(e.message);
        }
        if (once) return { code: 0 };
        sleep(INTERVAL_MS);
        // The lock's age says the loop is alive.
        utimesSync(lockPath, new Date(now()), new Date(now()));
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
