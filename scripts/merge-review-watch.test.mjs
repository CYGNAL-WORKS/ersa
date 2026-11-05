// Tests for merge-review-watch.mjs, run without GitHub, git or the review.
// Run: node --test scripts/merge-review-watch.test.mjs

import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

import { INTERVAL_MS, LOCK_STALE_MS, STALE_MS, lock, main, select } from "./merge-review-watch.mjs";

const HEAD = "b".repeat(40);
const NOW = Date.parse("2026-11-04T12:00:00Z");
const ago = (ms) => new Date(NOW - ms).toISOString();
const ci = (over) => ({ __typename: "CheckRun", name: "CI", status: "COMPLETED", conclusion: "SUCCESS", startedAt: ago(20 * 60 * 1000), ...over });
const mark = (state, over) => ({ __typename: "StatusContext", context: "Merge review", state, startedAt: ago(10 * 60 * 1000), ...over });
const pr = (over) => ({ number: 7, isDraft: false, isCrossRepository: false, headRefOid: HEAD, autoMergeRequest: null, statusCheckRollup: [ci()], ...over });

test("a same-repository, non-draft pull request whose CI passed on an unreviewed head is reviewed", () => {
  assert.deepEqual(select([pr()], NOW), { review: [{ number: 7, head: HEAD }], disarm: [], skip: [] });
  // CI read as the gate reads it: the latest run of each check, skipped and neutral pass, a commit status counts.
  assert.deepEqual(select([pr({ statusCheckRollup: [ci({ conclusion: "FAILURE" }), ci({ startedAt: ago(60 * 1000) }), ci({ name: "W", conclusion: "SKIPPED" }), { context: "ext", state: "SUCCESS" }] })], NOW).review.length, 1);
});

test("a fork's pull request is never reviewed", () => {
  assert.deepEqual(select([pr({ isCrossRepository: true })], NOW).review, []);
});

test("a draft is not reviewed", () => {
  assert.deepEqual(select([pr({ isDraft: true })], NOW).review, []);
});

test("a head whose CI failed, is still running or never ran is not reviewed", () => {
  assert.deepEqual(select([pr({ statusCheckRollup: [ci({ conclusion: "FAILURE" })] })], NOW).review, []);
  assert.deepEqual(select([pr({ statusCheckRollup: [ci(), ci({ name: "W", status: "IN_PROGRESS", conclusion: "" })] })], NOW).review, []);
  assert.deepEqual(select([pr({ statusCheckRollup: [] })], NOW).review, []);
  assert.deepEqual(select([pr({ statusCheckRollup: [mark("SUCCESS")] })], NOW).review, [], "the review's own status is not CI");
});

test("a head that carries a merge review is not reviewed again, whatever the verdict", () => {
  for (const state of ["SUCCESS", "FAILURE", "ERROR", "PENDING"]) assert.deepEqual(select([pr({ statusCheckRollup: [ci(), mark(state)] })], NOW).review, [], state);
  // Only the latest status of the context counts: a run that died and was rerun by hand.
  assert.deepEqual(select([pr({ statusCheckRollup: [ci(), mark("PENDING", { startedAt: ago(2 * STALE_MS) }), mark("SUCCESS")] })], NOW).review, []);
});

test("a pending merge review older than an hour is a run that died, and the head is reviewed again", () => {
  assert.deepEqual(select([pr({ statusCheckRollup: [ci(), mark("PENDING", { startedAt: ago(STALE_MS + 1000) })] })], NOW).review, [{ number: 7, head: HEAD }]);
  assert.deepEqual(select([pr({ statusCheckRollup: [ci(), mark("PENDING", { startedAt: ago(STALE_MS - 1000) })] })], NOW).review, []);
  assert.deepEqual(select([pr({ statusCheckRollup: [ci(), mark("PENDING", { startedAt: undefined })] })], NOW).review, [{ number: 7, head: HEAD }], "a pending status with no time is stale, not reviewed forever");
});

test("every open pull request not reviewed this tick says why, and a tick with nothing to review logs it", () => {
  const prs = [
    pr({ number: 1, isDraft: true }),
    pr({ number: 2, isCrossRepository: true }),
    pr({ number: 3, statusCheckRollup: [ci({ status: "IN_PROGRESS", conclusion: "" })] }),
    pr({ number: 4, statusCheckRollup: [ci({ conclusion: "FAILURE" })] }),
    pr({ number: 5, statusCheckRollup: [] }),
    ...["SUCCESS", "FAILURE", "ERROR", "PENDING"].map((state, i) => pr({ number: 6 + i, statusCheckRollup: [ci(), mark(state)] })),
  ];
  assert.deepEqual(select(prs, NOW).skip, [
    { number: 1, why: "it is a draft" },
    { number: 2, why: "it is from a fork" },
    { number: 3, why: "CI is still running: CI" },
    { number: 4, why: "CI did not pass: CI" },
    { number: 5, why: "it has no CI checks" },
    { number: 6, why: "its merge review passed" },
    { number: 7, why: "its merge review blocks the merge" },
    { number: 8, why: "its merge review failed to run" },
    { number: 9, why: "its merge review is running or waits for the gate" },
  ]);
  assert.deepEqual(select([pr()], NOW).skip, [], "a head due for review is not also skipped");
  assert.deepEqual(drive(["--once"], { prs: prs.slice(2, 4) }).logs, ["nothing to review: #3 (CI is still running: CI); #4 (CI did not pass: CI)"]);
  assert.deepEqual(drive(["--once"], { prs: [] }).logs, ["nothing to review"]);
});

test("auto-merge on an unreviewed head is disarmed, reviewed or not yet reviewable; on a reviewed head it is the review's own", () => {
  const armed = { enabledAt: ago(1000) };
  assert.deepEqual(select([pr({ autoMergeRequest: armed })], NOW).disarm, [7]);
  assert.deepEqual(select([pr({ autoMergeRequest: armed, statusCheckRollup: [ci({ status: "IN_PROGRESS", conclusion: "" })] })], NOW).disarm, [7], "disarmed before CI finishes, so the queue never gets it");
  assert.deepEqual(select([pr({ autoMergeRequest: armed, statusCheckRollup: [ci(), mark("SUCCESS")] })], NOW).disarm, []);
  assert.deepEqual(select([pr({ autoMergeRequest: armed, isCrossRepository: true })], NOW).disarm, [], "a fork's pull request is left to the owner");
  // A hold put on after the head was enqueued still holds it.
  assert.deepEqual(select([pr({ autoMergeRequest: armed, labels: [{ name: "hold" }], statusCheckRollup: [ci(), mark("SUCCESS")] })], NOW).disarm, [7]);
  assert.deepEqual(select([pr({ labels: [{ name: "hold" }], statusCheckRollup: [ci(), mark("SUCCESS")] })], NOW).disarm, []);
});

// The lock in a temporary folder, with the liveness check stood in for.
let home;
const tmp = () => (home ??= mkdtempSync(join(tmpdir(), "merge-review-watch-")));
after(() => home && rmSync(home, { recursive: true, force: true }));

test("the lock is held by one tick at a time, survives a dead or three-hour-old holder, and is released", () => {
  const path = join(tmp(), "a", "lock");
  const alive = (pid) => pid === 100;
  const release = lock(path, { alive, pid: 100 });
  assert.equal(readFileSync(path, "utf8"), "100");
  assert.equal(lock(path, { alive, pid: 200 }), null, "a second tick finds the lock held");
  release();
  assert.ok(!existsSync(path));
  writeFileSync(path, "300");
  const taken = lock(path, { alive, pid: 200 });
  assert.ok(taken, "a lock whose process is gone is taken over");
  assert.equal(readFileSync(path, "utf8"), "200");
  assert.deepEqual(readdirSync(join(tmp(), "a")), ["lock"], "the dead lock is gone, not left beside the new one");
  taken();
  // A pid Windows gave to another process reads as live; the file's age settles it.
  writeFileSync(path, "100");
  assert.equal(lock(path, { alive, pid: 200, now: () => Date.now() + LOCK_STALE_MS - 60_000 }), null, "a live lock under three hours old is held");
  const old = lock(path, { alive, pid: 200, now: () => Date.now() + LOCK_STALE_MS + 1000 });
  assert.ok(old, "a live-looking lock three hours old is taken over");
  assert.equal(LOCK_STALE_MS, 3 * STALE_MS, "past the two hours the task allows a tick");
  old();
  // A release removes only its own lock: a tick taken over must not remove its successor's.
  writeFileSync(path, "400");
  old();
  assert.equal(readFileSync(path, "utf8"), "400");
  rmSync(path);
  old();
  writeFileSync(path, "");
  assert.equal(lock(path, { alive, pid: 200 }), null, "a lock that names no pid is left for a person");
  assert.ok(existsSync(path));
});

test("a dead lock another tick replaced between the read and the rename goes back, and the lock is theirs", () => {
  const path = join(tmp(), "b", "lock");
  mkdirSync(join(tmp(), "b"), { recursive: true });
  writeFileSync(path, "300");
  // The other tick wins the race in the instant before this one's rename: the liveness check is where this one stalls.
  const alive = (pid) => {
    if (pid === 300) writeFileSync(path, "500");
    return pid === 500;
  };
  assert.equal(lock(path, { alive, pid: 200 }), null);
  assert.equal(readFileSync(path, "utf8"), "500", "the live lock is back in place");
  assert.deepEqual(readdirSync(join(tmp(), "b")), ["lock"]);
});

// The poller's own repository, where the worktree is added from.
const ROOT = join(fileURLToPath(import.meta.url), "..", "..");

// main() with every tool stood in for: gh lists `prs`, git answers from
// `git`, the review exits `exit`; `exists` says whether the checkout is there.
function drive(argv, { prs = [pr()], exit = 0, git = () => ({ ok: true }), list = () => null, lockPath, alive, sleep, now = () => NOW } = {}) {
  const calls = [];
  const logs = [];
  const ok = (out = "") => ({ ok: true, status: 0, out, err: "" });
  const run = (cmd, args, cwd, opts) => {
    calls.push({ cmd, args, cwd, live: opts?.live ?? false });
    if (cmd === "gh" && args[1] === "list") return list() ?? ok(JSON.stringify(prs));
    if (cmd === "gh") return ok();
    if (cmd === "git") return { ok: true, status: 0, out: "", err: "", ...git(args, cwd) };
    if (cmd === "node") {
      const status = typeof exit === "function" ? exit(args[1]) : exit;
      return { ok: status === 0, status, out: "", err: "" };
    }
    throw new Error(`unexpected ${cmd}`);
  };
  const path = lockPath ?? join(tmp(), `lock-${Math.random().toString(36).slice(2)}`);
  const got = main(argv, { run, now, log: (l) => logs.push(l), lockPath: path, alive, sleep });
  return { ...got, calls, logs, lockPath: path, reviews: calls.filter((c) => c.cmd === "node"), gh: calls.filter((c) => c.cmd === "gh").map((c) => c.args.join(" ")) };
}

test("one tick prepares the review checkout at the head, with main's review logic, and runs the review there", () => {
  const dir = join(tmp(), "review");
  const got = drive(["--once", "--checkout", dir]);
  assert.equal(got.code, 0, got.stderr);
  const git = got.calls.filter((c) => c.cmd === "git").map((c) => [c.args.join(" "), c.cwd]);
  assert.deepEqual(git, [
    ["worktree prune", ROOT],
    [`worktree add --detach ${dir} origin/main`, ROOT],
    ["fetch -q origin main pull/7/head", dir],
    [`reset -q --hard ${HEAD}`, dir],
    ["clean -q -fd", dir],
    // Ignored files under any .claude folder too: a settings.local.json the last review's tests wrote is not main's.
    ["clean -q -ffdx -- :(glob)**/.claude/**", dir],
    ["checkout -q origin/main -- scripts/merge-review.mjs scripts/merge-pr.mjs", dir],
  ]);
  assert.deepEqual(got.reviews.map((c) => [c.args.join(" "), c.cwd, c.live]), [[`scripts/merge-review.mjs 7 --head ${HEAD}`, dir, true]]);
  assert.deepEqual(got.gh, ["pr list --state open --limit 100 --json number,isDraft,isCrossRepository,headRefOid,labels,autoMergeRequest,statusCheckRollup"]);
  assert.match(got.logs.at(-1), /#7 at bbbbbbb: the review exited 0/);
  assert.ok(!existsSync(got.lockPath), "the lock is released");
});

test("an existing checkout is reused, one head is reviewed per tick, and a review that blocks or fails is reported", () => {
  const dir = join(tmp(), "existing");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, ".git"), "gitdir: elsewhere");
  const got = drive(["--once", "--checkout", dir], { prs: [pr(), pr({ number: 8, headRefOid: "c".repeat(40) })], exit: 1 });
  assert.equal(got.code, 0, got.stderr);
  assert.ok(!got.calls.some((c) => c.cmd === "git" && c.args[0] === "worktree"));
  // One review per tick, so a tick ends well within the two hours the task allows; the rest wait for the next.
  assert.deepEqual(got.reviews.map((c) => c.args[1]), ["7"]);
  assert.match(got.logs.join("\n"), /#7 at bbbbbbb: reviewing; #8 next\n#7 at bbbbbbb: the review exited 1/);
});

test("a head whose checkout cannot be prepared gives the tick to the next head; the first review run, whatever its exit, ends the tick", () => {
  // #7's checkout cannot be prepared; #8's review runs and fails (exit 2, which can come after a whole claude run); #9 waits.
  const prs = [pr(), pr({ number: 8, headRefOid: "c".repeat(40) }), pr({ number: 9, headRefOid: "d".repeat(40) })];
  const got = drive(["--once", "--checkout", join(tmp(), "r2")], {
    prs,
    git: (args) => (args[0] === "fetch" && args.includes("pull/7/head") ? { ok: false, err: "network down" } : {}),
    exit: 2,
  });
  assert.equal(got.code, 0, got.stderr);
  assert.deepEqual(got.reviews.map((c) => c.args[1]), ["8"]);
  assert.match(got.logs.join("\n"), /#7 at bbbbbbb: git fetch -q failed: network down\n#8 at ccccccc: reviewing; #9 next\n#8 at ccccccc: the review exited 2$/);
});

test("an hour of heads that cannot start ends the tick, so it stays inside the lock's age", () => {
  // Each failing prepare takes 25 minutes of the clock; the fourth head finds the hour gone.
  let t = NOW;
  const prs = [pr(), pr({ number: 8, headRefOid: "c".repeat(40) }), pr({ number: 9, headRefOid: "d".repeat(40) }), pr({ number: 10, headRefOid: "e".repeat(40) })];
  const got = drive(["--once", "--checkout", join(tmp(), "r7")], {
    prs,
    git: (args) => (args[0] === "fetch" ? ((t += 25 * 60 * 1000), { ok: false, err: "timed out" }) : {}),
    now: () => t,
  });
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviews.length, 0);
  assert.equal(got.calls.filter((c) => c.cmd === "git" && c.args[0] === "fetch").length, 3, "the fourth head is not tried");
  assert.match(got.logs.at(-1), /^the tick is an hour old; #10 wait for the next$/);
});

test("auto-merge on an unreviewed head is disabled before anything else", () => {
  const got = drive(["--once", "--checkout", join(tmp(), "r3")], { prs: [pr({ autoMergeRequest: { enabledAt: "x" }, statusCheckRollup: [ci({ status: "IN_PROGRESS", conclusion: "" })] })] });
  assert.deepEqual(got.gh.slice(1), ["pr merge 7 --disable-auto"]);
  assert.equal(got.reviews.length, 0);
  assert.match(got.logs[0], /#7: auto-merge was on for an unreviewed or held head; disabled it/);
});

test("two overlapping ticks: the second finds the lock held and does nothing", () => {
  const path = join(tmp(), "shared-lock");
  writeFileSync(path, "4242");
  const got = drive(["--once", "--checkout", join(tmp(), "r4")], { lockPath: path, alive: (pid) => pid === 4242 });
  assert.equal(got.code, 0);
  assert.match(got.stdout, /another tick holds .*shared-lock; nothing to do/);
  assert.equal(got.calls.length, 0, "no gh, git or review ran");
  assert.equal(readFileSync(path, "utf8"), "4242", "the other tick's lock is untouched");
});

test("without --once the poller ticks, waits five minutes, and ticks again, through a tool that is down for one tick; stopped, it releases the lock", () => {
  const waits = [];
  const stop = new Error("stop");
  let lists = 0;
  const got = drive(["--checkout", join(tmp(), "r5")], {
    // gh is down on the second tick only.
    list: () => (++lists === 2 ? { ok: false, status: 1, out: "", err: "gh: connection reset" } : null),
    sleep: (ms) => {
      waits.push(ms);
      if (waits.length === 3) throw stop;
    },
  });
  assert.equal(got.code, 2);
  assert.deepEqual(waits, [INTERVAL_MS, INTERVAL_MS, INTERVAL_MS]);
  assert.equal(got.reviews.length, 2, "one review per tick, none on the tick gh was down");
  assert.match(got.logs.join("\n"), /gh pr list failed: gh: connection reset/);
  assert.ok(!existsSync(got.lockPath));
  assert.equal(INTERVAL_MS, 5 * 60 * 1000);
});

test("a loop whose lock was taken over stops rather than run beside the new tick, and leaves that tick's lock alone", () => {
  const path = join(tmp(), "taken-lock");
  let naps = 0;
  const got = drive(["--checkout", join(tmp(), "r8")], {
    lockPath: path,
    // Another tick took the lock over while this loop slept; a loop that sleeps again did not notice.
    sleep: () => {
      if (++naps > 1) throw new Error("the loop went on beside the other tick");
      writeFileSync(path, "999");
    },
  });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /the lock at .*taken-lock is another tick's \(pid 999\), so this loop stops/);
  assert.equal(got.reviews.length, 1, "no second tick ran");
  assert.equal(readFileSync(path, "utf8"), "999", "the other tick's lock is not removed on the way out");
  rmSync(path);
});

test("with --once a tool that is down ends the tick with exit 2", () => {
  const got = drive(["--once", "--checkout", join(tmp(), "r6")], { list: () => ({ ok: false, status: 1, out: "", err: "gh: connection reset" }) });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /gh pr list failed: gh: connection reset/);
  assert.ok(!existsSync(got.lockPath));
});

test("a usage error exits 2 before anything runs", () => {
  for (const argv of [["--twice"], ["--checkout"], ["--once", "extra"]]) {
    const got = drive(argv);
    assert.equal(got.code, 2, argv.join(" "));
    assert.match(got.stderr, /usage/);
    assert.equal(got.calls.length, 0);
  }
});
