#!/usr/bin/env node
// The merge review poller (ADR 0023): on the owner's machine, with the owner's
// gh and Claude Code logins, it finds every open pull request from this
// repository whose CI passed on a head that carries no merge review yet, and
// runs scripts/merge-review.mjs on up to two of them at once, the head whose
// CI passed first leading.
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
// that may pass now), and the head is taken up again. An error status (the
// run itself failed) is retried after RETRY_MS, one such head a tick, for
// RETRY_UNTIL_MS after CI passed. Heads are due in the
// order their CI passed, so a pull request pushed again and again never keeps
// an older one waiting. Each tick starts a review in every free slot (SLOTS of
// them, each with its own checkout) and leaves it running: in a terminal the
// next tick fills a slot whose review has ended. --once does the same while
// its reviews run, starting none after its first START_WINDOW_MS, then waits
// for them, so a run ends within the two hours the task allows it. A pull
// request under review waits for that review to end, whatever its head now. A
// head whose checkout cannot be prepared tries each free slot, is logged, and
// the next head goes; in a terminal a tick gives such heads an hour at most. A tick with no head to review
// logs each open pull request it passed over and why, so a quiet log says
// what it waits for. A head without the status that
// GitHub would merge on its own (auto-merge armed by a review of an earlier
// head, which a push by the owner's account does not disarm), or any head of a
// pull request labeled `hold`, gets auto-merge disabled first, so nothing
// unreviewed or held enters the queue. A lock file in .scratch/ holds the pid
// of the running tick, so two ticks never overlap; a lock whose process is gone
// or that is three hours old is taken over.
//
// Each review runs in its slot's checkout, a detached worktree of this
// repository at <dir> for the first slot (by default a sibling folder named
// after this one with "-review" appended) and <dir>-2 for the second, reset to
// the head with the review script and the gate taken from
// main, and every file under a .claude folder removed, ignored ones included;
// the review script takes the hooks and the instruction files from main itself. The review runs the pull request's own
// tests as the owner, with the owner's gh login in the environment: that is
// what "never a fork" protects, and why the repository's default workflow
// token stays read-only, so a pull request's own workflow cannot post the
// status or a review.
//
// The scheduled task, registered once from a PowerShell prompt in the
// repository, with node, gh and claude on the user's PATH. It runs only while
// the owner is logged on, since both logins are theirs, and appends the
// poller's lines and each review's output, every line marked with its pull
// request, to .scratch\merge-review-watch.log,
// making the folder first on a fresh clone (cmd would bind the `&` inside an
// `if`, so the mkdir runs unconditionally and its complaint is dropped):
//
//   $run = New-ScheduledTaskAction -Execute "cmd" -Argument "/c mkdir .scratch 2>nul & node scripts\merge-review-watch.mjs --once >> .scratch\merge-review-watch.log 2>&1" -WorkingDirectory (Get-Location)
//   $every5 = New-ScheduledTaskTrigger -Once -At (Get-Date) -RepetitionInterval (New-TimeSpan -Minutes 5)
//   $limits = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit (New-TimeSpan -Hours 2)
//   Register-ScheduledTask -TaskName "Ersa merge review" -Action $run -Trigger $every5 -Settings $limits
//
// Exit 0 after a tick, or at once when another tick holds the lock; 2 on a
// usage or tool error.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, linkSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, utimesSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

import { MERGE_REVIEW, ciRefusals, normalizeChecks } from "./merge-pr.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const LOCK = join(ROOT, ".scratch", "merge-review-watch.lock");
const CHECKOUT = join(ROOT, "..", `${basename(ROOT)}-review`);
export const INTERVAL_MS = 5 * 60 * 1000;
// Reviews that run at once. Each builds and tests a pull request on this
// machine, so more would slow every one of them down.
export const SLOTS = 2;
// A pending status older than this belongs to a run that is over: longer than
// the review's claude runs (38 and 4 minutes) and its tool calls at their
// usual pace. A lock gets three times that, past the two hours the task
// allows a run: a run is at most START_WINDOW_MS of heads that cannot start
// plus the reviews it started, which run side by side, each with every tool
// call stalled to its two-minute limit, and the task's
// stop may end only the shell while the review runs on, so a live tick must
// never be taken over.
export const STALE_MS = 60 * 60 * 1000;
export const LOCK_STALE_MS = 3 * STALE_MS;
// A review whose own run failed (its status is an error, say queueing hit
// GitHub's rate limit) is retried this long after the status was set. Retries
// go behind every head due for its first review, one a tick at most, so a head
// that always errors never takes every slot. Retries end once the head's CI
// passed RETRY_UNTIL_MS ago, which no retry resets; the error is then the owner's.
export const RETRY_MS = 15 * 60 * 1000;
export const RETRY_UNTIL_MS = 6 * 60 * 60 * 1000;
// How long a run of the scheduled task starts reviews for, counted from its
// start, heads that cannot be prepared included: a review takes about an hour,
// so the last one it starts ends within the two hours the task allows it.
export const START_WINDOW_MS = 45 * 60 * 1000;
// How long a review may run before it is ended. A review takes up to about an
// hour; one started at the end of the window and ended here still finishes
// within the task's two hours.
export const REVIEW_LIMIT_MS = 70 * 60 * 1000;
// What a review that was ended at its limit resolves to.
export const ENDED = "nothing: it ran past its time limit and was ended";
const FIELDS = "number,isDraft,isCrossRepository,headRefOid,labels,autoMergeRequest,statusCheckRollup";

// What a head's merge review status says, for the line that tells why the
// head is not reviewed again.
const VERDICT = { SUCCESS: "passed", FAILURE: "blocks the merge", ERROR: "failed to run", PENDING: "is running or waits for the gate" };

// Which pull requests a tick acts on: `review` the heads to review, the one
// whose CI passed first leading, `disarm` the numbers whose auto-merge must go, and `skip` every other
// open pull request with why it is not reviewed, so a tick with nothing to
// review says what it is waiting for. Pure, so a test can drive every case
// with gh's JSON alone.
export function select(prs, now = Date.now()) {
  const review = [];
  const disarm = [];
  const skip = [];
  for (const pr of prs) {
    if (pr.isDraft || pr.isCrossRepository) {
      skip.push({ number: pr.number, why: pr.isDraft ? "it is a draft" : "it is from a fork" });
      continue;
    }
    const mark = normalizeChecks(pr.statusCheckRollup).find((c) => c.name === MERGE_REVIEW);
    // A pending time gh did not give reads as stale: the lock keeps a running review safe from the retake.
    const stale = mark?.status === "PENDING" && !(now - Date.parse(mark.startedAt ?? "") <= STALE_MS);
    const retry = mark?.status === "COMPLETED" && mark.conclusion === "ERROR" && !(now - Date.parse(mark.startedAt ?? "") < RETRY_MS) && now - passedAt(pr.statusCheckRollup, -Infinity) < RETRY_UNTIL_MS;
    const reviewed = Boolean(mark) && !stale && !retry;
    const held = (pr.labels ?? []).some((l) => l.name === "hold");
    if (pr.autoMergeRequest && (!reviewed || held)) disarm.push(pr.number);
    const ci = ciRefusals(pr.statusCheckRollup);
    if (reviewed) skip.push({ number: pr.number, why: `its merge review ${VERDICT[mark.status === "COMPLETED" ? mark.conclusion : "PENDING"] ?? "has a status this poller cannot read"}${held ? ", and it is on hold" : ""}` });
    else if (ci.length) skip.push({ number: pr.number, why: ci.join(", and ") });
    else review.push({ number: pr.number, head: pr.headRefOid, retry, since: passedAt(pr.statusCheckRollup, now) });
  }
  review.sort((a, b) => a.retry - b.retry || a.since - b.since || a.number - b.number);
  const firstRetry = review.findIndex((r) => r.retry);
  if (firstRetry >= 0) for (const r of review.splice(firstRetry + 1)) skip.push({ number: r.number, why: "its merge review failed to run, and another failed head is retried this tick" });
  return { review: review.map(({ number, head }) => ({ number, head })), disarm, skip };
}

// When a head's CI passed: the end of its last check to finish, among those gh
// gave a time for. A head with no time at all counts as passing now, so a
// missing time neither jumps the line nor keeps a head at its back for good.
function passedAt(rollup, now) {
  const times = normalizeChecks(rollup)
    .filter((c) => c.name !== MERGE_REVIEW)
    .map((c) => Date.parse(c.completedAt ?? c.startedAt ?? ""))
    .filter(Number.isFinite);
  return times.length ? Math.max(...times) : now;
}

// Takes the lock for this process and returns what releases it, or null when
// a live tick holds it. A lock is a tick that died when its pid is gone or
// the file is older than LOCK_STALE_MS (a pid Windows gave to another process
// reads as live, so age settles it). It is taken over by renaming it away,
// which only one of two ticks reading the same dead lock can do; a renamed
// file that no longer holds the pid that was read is a live lock another tick
// wrote in between, and goes back by a hard link, which fails rather than
// replace a lock a third tick wrote meanwhile (that tick then holds it, and
// the second one's loop notices at its next refresh). The release removes the
// lock only while it still holds this pid. A lock that names no pid is left
// for a person.
export function lock(path, { alive = isAlive, pid = process.pid, now = Date.now } = {}) {
  mkdirSync(dirname(path), { recursive: true });
  for (;;) {
    try {
      writeFileSync(path, String(pid), { flag: "wx" });
      return () => {
        try {
          if (readFileSync(path, "utf8") === String(pid)) rmSync(path, { force: true });
        } catch (e) {
          if (e.code !== "ENOENT") throw e;
        }
      };
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
    if (alive(holder) && age < LOCK_STALE_MS) return null;
    const gone = `${path}.${pid}.stale`;
    try {
      renameSync(path, gone);
    } catch (e) {
      if (e.code === "ENOENT") continue;
      throw e;
    }
    if (readFileSync(gone, "utf8") !== String(holder)) {
      try {
        linkSync(gone, path);
      } catch (e) {
        if (e.code !== "EEXIST") throw e;
      }
      rmSync(gone, { force: true });
      return null;
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

const ENV = { ...process.env, GIT_TERMINAL_PROMPT: "0", GH_PROMPT_DISABLED: "1" };

// Git and gh stall on a prompt or a dead network.
function run(cmd, args, cwd = ROOT) {
  const r = spawnSync(cmd, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, timeout: 2 * 60 * 1000, env: ENV });
  return { ok: r.status === 0, status: r.status, out: (r.stdout ?? "").trim(), err: [(r.stderr ?? "").trim(), r.error?.message].filter(Boolean).join("; ") };
}

// Starts the review of pull request `n` at `head` in `dir` and resolves to its
// exit code, or why it could not start or did not end. Its output goes to
// `log` a line at a time, each marked with the pull request, so two reviews'
// lines stay readable side by side. A review that runs past `limit` is ended
// with every process it started: a run waits for its reviews and keeps its
// lock fresh meanwhile, so one that never ended would hold every other head.
// A process the review left behind can hold its output open after it exits,
// so the exit, not the end of the output, ends it. `launch` stands in for
// spawn, so a test can hand it a process that behaves any way.
export function startReview(n, head, dir, log, { limit = REVIEW_LIMIT_MS, launch = spawn } = {}) {
  return new Promise((resolve) => {
    let timer;
    const done = (status) => {
      clearTimeout(timer);
      resolve(status);
    };
    // Outside Windows the review leads its own process group, so it can be ended whole.
    const child = launch("node", ["scripts/merge-review.mjs", String(n), "--head", head], { cwd: dir, env: ENV, stdio: ["ignore", "pipe", "pipe"], detached: process.platform !== "win32" });
    // Control characters go, so the pull request's own output cannot redraw the log.
    for (const out of [child.stdout, child.stderr]) createInterface({ input: out }).on("line", (l) => log(`#${n} | ${l.replace(/[\x00-\x1f\x7f]/g, "")}`));
    timer = setTimeout(() => {
      endTree(child.pid);
      done(ENDED);
    }, limit);
    child.on("error", (e) => done(e.message));
    // Output still in the pipe gets a moment to reach the log.
    child.on("exit", (code, signal) => {
      const status = code ?? signal;
      child.on("close", () => done(status));
      setTimeout(() => done(status), 2000).unref();
    });
  });
}

// Ends a process and every process it started.
function endTree(pid) {
  if (process.platform === "win32") spawnSync("taskkill", ["/pid", String(pid), "/t", "/f"], { stdio: "ignore" });
  else {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {}
  }
}

const pause = (ms) => new Promise((wake) => setTimeout(wake, ms));
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
  must(["clean", "-q", "-ffdx", "--", ":(glob)**/.claude/**"], dir);
  must(["checkout", "-q", "origin/main", "--", "scripts/merge-review.mjs", "scripts/merge-pr.mjs"], dir);
}

// One pass: disarm what must not merge, then start a review in each free
// slot, taking the heads due in order. `running` maps each pull request under
// review to its slot's checkout and the review's end. A head whose checkout
// cannot be prepared is reported and the next head takes the slot, so one bad
// head does not hold the rest; no head is taken up after `until`.
function tick(run, start, dirs, log, now, running, until) {
  const list = run("gh", ["pr", "list", "--state", "open", "--limit", "100", "--json", FIELDS]);
  if (!list.ok) fail(`gh pr list failed: ${list.err}`);
  let prs;
  try {
    prs = JSON.parse(list.out);
  } catch (e) {
    fail(`gh pr list gave output that is not JSON: ${e.message}`);
  }
  const { review, disarm, skip } = select(prs, now());
  for (const n of disarm) {
    const r = run("gh", ["pr", "merge", String(n), "--disable-auto"]);
    log(`#${n}: auto-merge was on for an unreviewed or held head; ${r.ok ? "disabled it" : `could not disable it: ${r.err}`}`);
  }
  // Check names come from the pull request's own workflows: one line, whatever they hold.
  // A pull request under review waits for that review to end, whatever its head now.
  const due = review.filter((p) => !running.has(p.number));
  if (!due.length && !running.size) return log(`nothing to review${skip.length ? `: ${skip.map((p) => `#${p.number} (${p.why})`).join("; ")}` : ""}`.replace(/\s+/g, " "));
  const free = dirs.filter((d) => ![...running.values()].some((x) => x.dir === d));
  for (const [i, { number: n, head }] of due.entries()) {
    const later = due.slice(i + 1).map((p) => `#${p.number}`);
    if (!free.length) return log(`${[`#${n}`, ...later].join(", ")} wait for a free slot`);
    if (now() > until) return log(`out of time; ${[`#${n}`, ...later].join(", ")} wait for the next run`);
    log(`#${n} at ${head.slice(0, 7)}: reviewing${later.length ? `; ${later.join(", ")} next` : ""}`);
    // The head tries each free slot in turn: the fault may be one checkout's, not the head's.
    const dir = free.find((d) => {
      try {
        prepare(run, d, n, head);
        return true;
      } catch (e) {
        log(`#${n} at ${head.slice(0, 7)}: ${e.message}`);
        return false;
      }
    });
    if (!dir) continue;
    free.splice(free.indexOf(dir), 1);
    // A review that could not start ends like one that failed, so its slot and the lock are freed.
    const done = Promise.resolve()
      .then(() => start(n, head, dir, log))
      .catch((e) => e?.message ?? String(e))
      .then((status) => {
        running.delete(n);
        log(`#${n} at ${head.slice(0, 7)}: the review exited ${status}`);
        // An ended review never set its verdict, and its pending status would
        // go stale and send the head round again; an error is retried for a while.
        if (status !== ENDED) return;
        const description = `The review ran past ${REVIEW_LIMIT_MS / 60000} minutes and was ended; the poller's log says what it was doing.`;
        const r = run("gh", ["api", `repos/{owner}/{repo}/statuses/${head}`, "-f", "state=error", "-f", `context=${MERGE_REVIEW}`, "-f", `description=${description}`]);
        if (!r.ok) log(`#${n} at ${head.slice(0, 7)}: could not mark the head's merge review as failed: ${r.err}`);
      });
    running.set(n, { dir, done });
  }
}

const USAGE = "usage: node scripts/merge-review-watch.mjs [--once] [--checkout <dir>]";

// Runs the poller for argv. `run` runs one command, `start` starts a review,
// `sleep` waits between ticks, `now` tells the time and `log` takes each line,
// so a test can stand in for every tool and the clock.
export async function main(argv, { run: r = run, start = startReview, sleep = pause, now = Date.now, log = (l) => process.stdout.write(`${stamp()} ${l}\n`), lockPath = LOCK, alive } = {}) {
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
    const dirs = Array.from({ length: SLOTS }, (_, i) => (i ? `${dir}-${i + 1}` : dir));
    const running = new Map();
    const began = now();
    try {
      for (;;) {
        // A tool that is down for one tick is not the end of the loop, or of
        // a task's run with reviews to wait for; a task's run with none says
        // so and ends. In a terminal a tick gives heads that cannot be
        // prepared an hour; a task's run starts nothing after its window.
        try {
          tick(r, start, dirs, log, now, running, once ? began + START_WINDOW_MS : now() + STALE_MS);
        } catch (e) {
          if ((once && !running.size) || !(e instanceof ToolError)) throw e;
          log(e.message);
        }
        // The task's run ticks on while its reviews run, so a slot that frees
        // up is filled.
        if (once && !running.size) return { code: 0 };
        await sleep(INTERVAL_MS);
        // The lock's age says the loop is alive; a lock that is gone or
        // another tick's was taken over or removed by hand, and this loop
        // stops rather than run beside it.
        let holder = "";
        try {
          holder = readFileSync(lockPath, "utf8");
        } catch (e) {
          if (e.code !== "ENOENT") throw e;
        }
        if (holder !== String(process.pid)) fail(`the lock at ${lockPath} is ${holder ? `another tick's (pid ${holder})` : "gone"}, so this loop stops`);
        utimesSync(lockPath, new Date(now()), new Date(now()));
      }
    } finally {
      // The lock stays until every review this process started has ended, so
      // no other tick takes up a head still under review.
      await Promise.all([...running.values()].map((x) => x.done));
      release();
    }
  } catch (e) {
    return { code: 2, stderr: `${e instanceof ToolError ? e.message : e.stack}\n` };
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { code, stdout, stderr } = await main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
