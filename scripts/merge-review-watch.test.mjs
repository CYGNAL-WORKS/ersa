// Tests for merge-review-watch.mjs, run without GitHub, git or the review.
// Run: node --test scripts/merge-review-watch.test.mjs

import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { EventEmitter } from "node:events";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PassThrough } from "node:stream";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

import { INTERVAL_MS, LOCK_STALE_MS, ENDED, TRANSIENT, RETRY_MS, latestCause, RETRY_UNTIL_MS, SLOTS, STALE_MS, START_WINDOW_MS, lock, main, select, startReview } from "./merge-review-watch.mjs";

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

const QUEUE_FAILED = "The run failed: adding it to the merge queue failed: GraphQL: API rate limit already exceeded";
// The REST list of a head's statuses, newest first as GitHub gives it, with an older merge review and another context around it.
const said = (description) => ({ ok: true, out: JSON.stringify([{ context: "ext", description: "rate limit", created_at: ago(1000) }, { context: "Merge review", description, created_at: ago(2000) }, { context: "Merge review", description: "Reviewing…", created_at: ago(3000) }]) });
const errored = (over) => mark("ERROR", { startedAt: ago(RETRY_MS + 1000), ...over });

test("a merge review that failed for a cause that may clear is retried after a backoff, behind fresh heads", () => {
  assert.equal(RETRY_MS, 15 * 60 * 1000);
  assert.equal(RETRY_UNTIL_MS, 6 * 60 * 60 * 1000);
  const failed = (n, at) => pr({ number: n, statusCheckRollup: [ci(), errored({ startedAt: at })] });
  assert.deepEqual(select([failed(7, ago(RETRY_MS - 1000))], NOW).review, []);
  assert.deepEqual(select([failed(7, ago(RETRY_MS + 1000))], NOW).review, [{ number: 7, head: HEAD, retry: true }]);
  assert.deepEqual(select([failed(7, undefined)], NOW).review, [{ number: 7, head: HEAD, retry: true }], "an error with no time is retried, not left for good");
  assert.deepEqual(select([pr({ autoMergeRequest: { enabledAt: ago(1000) }, statusCheckRollup: [ci(), errored()] })], NOW).disarm, [7]);
  const picked = select([failed(3, ago(2 * RETRY_MS)), failed(4, ago(2 * RETRY_MS)), pr({ number: 9, statusCheckRollup: [ci({ startedAt: ago(1000) })] })], NOW);
  assert.deepEqual(picked.review.map((p) => p.number), [9, 3, 4]);
  assert.deepEqual(select([pr({ statusCheckRollup: [ci({ startedAt: ago(RETRY_UNTIL_MS + 1000) }), errored()] })], NOW).review, [], "a head that keeps failing is left to the owner");
  assert.deepEqual(select([pr({ statusCheckRollup: [{ context: "ext", state: "SUCCESS" }, errored()] })], NOW).review, [], "with no CI time the cap holds");
});

test("one retry runs at a time, counting a retry an earlier tick started", async () => {
  const prs = [3, 4].map((n) => pr({ number: n, headRefOid: String(n).repeat(40), statusCheckRollup: [ci(), errored()] }));
  const reviews = [held(), held()];
  let started = 0;
  const run = drive(["--once"], { prs, start: () => reviews[started++].done });
  await settle(() => started === 1);
  await settle(() => false).catch(() => {});
  assert.equal(started, 1, "the second failed head waits though a slot is free");
  reviews[0].end(0);
  await settle(() => started === 2);
  reviews[1].end(0);
  assert.equal((await run).code, 0);
});

test("TRANSIENT matches each cause that may clear on its own, and nothing else", () => {
  for (const cause of [
    "GraphQL: API rate limit already exceeded for user ID 1",
    "Too Many Requests",
    "HTTP 500",
    "HTTP 502",
    "HTTP 408",
    "HTTP 429",
    "read tcp 10.0.0.1:5000->140.82.112.6:443: connection reset by peer",
    "dial tcp: i/o timeout",
    "error connecting to api.github.com",
    "network is unreachable",
    "The run failed: adding it to the merge queue failed after the review was posted: refused",
  ]) assert.match(cause, TRANSIENT);
  for (const cause of ["The review ran past 70 minutes and was ended; the poller's log says what it was doing.", "The run failed: claude returned no result: error_max_turns", "HTTP 404: Not Found", "HTTP 4290", "claude returned no result after 500 turns", ""]) assert.doesNotMatch(cause, TRANSIENT);
});

test("the cause read is the newest merge review status's, whatever order GitHub lists them in", () => {
  const status = (description, at) => ({ context: "Merge review", description, created_at: ago(at) });
  assert.equal(latestCause(JSON.stringify([status("old: claude returned no result", 9000), status(QUEUE_FAILED, 1000)])), QUEUE_FAILED);
  assert.equal(latestCause(JSON.stringify([status(QUEUE_FAILED, 9000), status("new: claude returned no result", 1000)])), "new: claude returned no result");
  assert.equal(latestCause(JSON.stringify([{ context: "ext", description: "rate limit", created_at: ago(1) }])), "");
  assert.equal(latestCause("not json"), null);
  assert.equal(latestCause("{}"), null);
});

test("a retry starts only when its error's description says the cause may clear", async () => {
  const prs = [pr({ statusCheckRollup: [ci(), errored()] })];
  for (const description of [QUEUE_FAILED, "The run failed: gh api --paginate repos/o/r/pulls/7/reviews failed: HTTP 502: Bad Gateway", "posting the review failed 3 times: read tcp: connection reset by peer"])
    assert.equal((await drive(["--once"], { prs, statuses: () => said(description) })).reviews.length, 1, description);
  for (const description of ["The review ran past 70 minutes and was ended; the poller's log says what it was doing.", "The run failed: claude returned no result: error_max_turns", ""]) {
    const got = await drive(["--once"], { prs, statuses: () => said(description) });
    assert.equal(got.reviews.length, 0, description);
    assert.match(got.logs.join("\n"), /does not clear on its own/);
  }
  const down = await drive(["--once"], { prs, statuses: () => ({ ok: false, out: "", err: "HTTP 403: API rate limit exceeded" }) });
  assert.equal(down.reviews.length, 0, "an unreadable cause is not retried this tick");
  assert.match(down.logs.join("\n"), /could not read why its merge review failed: HTTP 403/);
});

test("every open pull request not reviewed this tick says why, and a tick with nothing to review logs it", async () => {
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
  assert.deepEqual(select([pr({ statusCheckRollup: [ci({ conclusion: "FAILURE" }), ci({ name: "W", status: "IN_PROGRESS", conclusion: "" })] })], NOW).skip, [{ number: 7, why: "CI is still running: W, and CI did not pass: CI" }]);
  assert.deepEqual(select([pr({ labels: [{ name: "hold" }], statusCheckRollup: [ci(), mark("SUCCESS")] })], NOW).skip, [{ number: 7, why: "its merge review passed, and it is on hold" }]);
  // A merge review mark gh gives in another shape: in progress as a check run, or a status with no state.
  assert.deepEqual(select([pr({ statusCheckRollup: [ci(), { __typename: "CheckRun", name: "Merge review", status: "IN_PROGRESS", conclusion: "", startedAt: ago(1000) }] })], NOW).skip, [{ number: 7, why: "its merge review is running or waits for the gate" }]);
  assert.deepEqual(select([pr({ statusCheckRollup: [ci(), { __typename: "StatusContext", context: "Merge review", startedAt: ago(1000) }] })], NOW).skip, [{ number: 7, why: "its merge review has a status this poller cannot read" }]);
  assert.deepEqual(select([pr()], NOW).skip, [], "a head due for review is not also skipped");
  assert.deepEqual((await drive(["--once"], { prs: prs.slice(2, 4) })).logs, ["nothing to review: #3 (CI is still running: CI); #4 (CI did not pass: CI)"]);
  assert.deepEqual((await drive(["--once"], { prs: [] })).logs, ["nothing to review"]);
  assert.deepEqual((await drive(["--once"], { prs: [pr({ statusCheckRollup: [ci({ name: "a\nb", conclusion: "FAILURE" })] })] })).logs, ["nothing to review: #7 (CI did not pass: a b)"], "a check name cannot break the line");
});

test("auto-merge on an unreviewed head is disarmed, reviewed or not yet reviewable; on a reviewed head it is the review's own", () => {
  const armed = { enabledAt: ago(1000) };
  assert.deepEqual(select([pr({ autoMergeRequest: armed })], NOW), { review: [{ number: 7, head: HEAD }], disarm: [7], skip: [] });
  assert.deepEqual(select([pr({ autoMergeRequest: armed, statusCheckRollup: [ci({ status: "IN_PROGRESS", conclusion: "" })] })], NOW), { review: [], disarm: [7], skip: [{ number: 7, why: "CI is still running: CI" }] }, "disarmed before CI finishes, so the queue never gets it");
  assert.deepEqual(select([pr({ autoMergeRequest: armed, statusCheckRollup: [ci(), mark("SUCCESS")] })], NOW), { review: [], disarm: [], skip: [{ number: 7, why: "its merge review passed" }] });
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

// main() with every tool stood in for: gh lists `prs`, each head a review
// started carrying the status the review sets; git answers from `git`; a
// review ends with `exit` (or with what `start` resolves to, for a test that
// holds a review open); `reviews` lists each review started. A wait between
// ticks lets the test run and no more, unless `sleep` says otherwise.
async function drive(argv, { prs = [pr()], exit = 0, git = () => ({ ok: true }), list = () => null, statuses = () => said(QUEUE_FAILED), start, lockPath, alive, sleep = () => new Promise((r) => setImmediate(r)), now = () => NOW } = {}) {
  const calls = [];
  const logs = [];
  const reviews = [];
  const ok = (out = "") => ({ ok: true, status: 0, out, err: "" });
  const marked = (p) => (reviews.some((r) => r.head === p.headRefOid) ? { ...p, statusCheckRollup: [...p.statusCheckRollup, mark("SUCCESS")] } : p);
  const run = (cmd, args, cwd) => {
    calls.push({ cmd, args, cwd });
    if (cmd === "gh" && args[1] === "list") return list() ?? ok(JSON.stringify(prs.map(marked)));
    if (cmd === "gh" && /\/commits\/.*\/statuses$/.test(args[1] ?? "")) return { status: 0, err: "", ...statuses() };
    if (cmd === "gh") return ok();
    if (cmd === "git") return { ok: true, status: 0, out: "", err: "", ...git(args, cwd) };
    throw new Error(`unexpected ${cmd}`);
  };
  const begin = (n, head, dir, log) => {
    reviews.push({ n: String(n), head, dir });
    return start ? start(n, head, dir, log) : Promise.resolve(exit);
  };
  const path = lockPath ?? join(tmp(), `lock-${Math.random().toString(36).slice(2)}`);
  const got = await main(argv, { run, start: begin, now, log: (l) => logs.push(l), lockPath: path, alive, sleep });
  return { ...got, calls, logs, reviews, lockPath: path, gh: calls.filter((c) => c.cmd === "gh").map((c) => c.args.join(" ")) };
}

// Lets the poller run until `ready` holds, a few turns of the event loop at most.
async function settle(ready = () => true) {
  for (let i = 0; i < 50; i++) {
    await new Promise((r) => setImmediate(r));
    if (i >= 2 && ready()) return;
  }
  throw new Error("the poller never got there");
}

// A review that ends only when the test says so.
function held() {
  let end;
  const done = new Promise((r) => (end = r));
  return { done, end };
}

test("one tick prepares the review checkout at the head, with main's review logic, and runs the review there", async () => {
  const dir = join(tmp(), "review");
  const got = await drive(["--once", "--checkout", dir]);
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
  assert.deepEqual(got.reviews, [{ n: "7", head: HEAD, dir }]);
  assert.equal(got.gh[0], "pr list --state open --limit 100 --json number,isDraft,isCrossRepository,headRefOid,labels,autoMergeRequest,statusCheckRollup");
  // The run ticks once more while its review runs, finds nothing new, and ends with the review.
  assert.deepEqual(got.logs, ["#7 at bbbbbbb: reviewing", "#7 at bbbbbbb: the review exited 0", "nothing to review: #7 (its merge review passed)"]);
  assert.ok(!existsSync(got.lockPath), "the lock is released");
});

test("heads are due in the order their CI passed, not the order gh lists them", () => {
  const passed = (min) => [ci({ startedAt: ago((min + 5) * 60 * 1000), completedAt: ago(min * 60 * 1000) })];
  // gh lists the newest pull request first; #3's CI passed an hour ago, #9's a minute ago.
  const prs = [pr({ number: 9, statusCheckRollup: passed(1) }), pr({ number: 5, statusCheckRollup: passed(30) }), pr({ number: 3, statusCheckRollup: passed(60) })];
  assert.deepEqual(select(prs, NOW).review.map((p) => p.number), [3, 5, 9]);
  // Its last check to finish says when a head's CI passed.
  const late = pr({ number: 4, statusCheckRollup: [ci({ completedAt: ago(90 * 60 * 1000) }), ci({ name: "W", completedAt: ago(10 * 60 * 1000) })] });
  assert.deepEqual(select([late, pr({ number: 6, statusCheckRollup: passed(20) })], NOW).review.map((p) => p.number), [6, 4]);
  // A head with no time at all passes now; equal times go by number.
  assert.deepEqual(select([pr({ number: 2, statusCheckRollup: [ci({ startedAt: undefined })] }), pr({ number: 8, statusCheckRollup: passed(5) }), pr({ number: 1, statusCheckRollup: [ci({ startedAt: undefined })] })], NOW).review.map((p) => p.number), [8, 1, 2]);
  // One check with no time does not send a head to the back: the checks with a time say when it passed.
  assert.deepEqual(select([pr({ number: 9, statusCheckRollup: passed(1) }), pr({ number: 3, statusCheckRollup: [...passed(60), ci({ name: "W", startedAt: undefined })] })], NOW).review.map((p) => p.number), [3, 9]);
  // The merge review's own status is not CI: #3 passed CI three hours ago and
  // #9 two, and #3's stale review status from 90 minutes ago does not count.
  assert.deepEqual(select([pr({ number: 9, statusCheckRollup: passed(120) }), pr({ number: 3, statusCheckRollup: [...passed(180), mark("PENDING", { startedAt: ago(90 * 60 * 1000) })] })], NOW).review.map((p) => p.number), [3, 9]);
});

test("two heads are reviewed side by side, each in its own slot's checkout, and the rest wait for a free slot", async () => {
  const dir = join(tmp(), "existing");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, ".git"), "gitdir: elsewhere");
  const prs = [pr(), pr({ number: 8, headRefOid: "c".repeat(40) }), pr({ number: 9, headRefOid: "d".repeat(40) })];
  const reviews = [held(), held(), held()];
  let started = 0;
  const run = drive(["--once", "--checkout", dir], { prs, start: () => reviews[started++].done });
  await settle();
  assert.equal(started, SLOTS, "the second review starts without waiting for the first");
  // #8's review ends while #7's runs: the task's run fills the freed slot with #9 on its next tick.
  reviews[1].end(1);
  await settle(() => started === 3);
  reviews[2].end(0);
  reviews[0].end(0);
  const got = await run;
  assert.equal(got.code, 0, got.stderr);
  assert.deepEqual(got.reviews.map((r) => [r.n, r.dir]), [["7", dir], ["8", `${dir}-2`], ["9", `${dir}-2`]]);
  // The first slot's checkout is reused; the second slot's is made.
  assert.deepEqual(got.calls.filter((c) => c.cmd === "git" && c.args[0] === "worktree").map((c) => c.args.join(" ")).slice(0, 2), ["worktree prune", `worktree add --detach ${dir}-2 origin/main`]);
  assert.ok(!got.calls.some((c) => c.cmd === "git" && c.args.includes(`${dir}`) && c.args[1] === "add"), "the existing checkout is not added again");
  assert.deepEqual(got.logs.slice(0, 3), ["#7 at bbbbbbb: reviewing; #8, #9 next", "#8 at ccccccc: reviewing; #9 next", "#9 wait for a free slot"]);
  assert.ok(got.logs.indexOf("#8 at ccccccc: the review exited 1") < got.logs.indexOf("#9 at ddddddd: reviewing"));
});

test("the task's run starts no review after its window, and then waits for those it started", async () => {
  // #7's review runs throughout. Each wait between ticks takes 25 minutes, and
  // a new head arrives with each: #8 at 25 minutes, #9 at 50, past the window.
  let t = NOW;
  let naps = 0;
  const first = held();
  const prs = [pr()];
  const got = await drive(["--once", "--checkout", join(tmp(), "r11")], {
    prs,
    start: (n) => (n === 7 ? first.done : Promise.resolve(0)),
    now: () => t,
    sleep: async () => {
      t += 25 * 60 * 1000;
      if (naps > 3) throw new Error("the run went on past its window");
      prs.push(pr({ number: 8 + naps, headRefOid: String(8 + naps).repeat(20) }));
      if (++naps === 2) first.end(0);
      await settle();
    },
  });
  assert.equal(got.code, 0, got.stderr);
  assert.deepEqual(got.reviews.map((r) => r.n), ["7", "8"], "#9 arrived after the window and waits for the next run");
  assert.equal(START_WINDOW_MS, 45 * 60 * 1000, "a review of about an hour still ends inside the task's two hours");
  assert.ok(!existsSync(got.lockPath));
});

test("with --once the lock is held until every review the tick started has ended", async () => {
  const review = held();
  const lockPath = join(tmp(), "once-lock");
  const run = drive(["--once", "--checkout", join(tmp(), "r9")], { start: () => review.done, lockPath });
  await new Promise((r) => setImmediate(r));
  assert.equal(await Promise.race([run, Promise.resolve("running")]), "running", "the tick has not returned while its review runs");
  assert.ok(existsSync(lockPath), "no other tick can take up the head under review");
  review.end(0);
  const got = await run;
  assert.equal(got.code, 0);
  assert.ok(!existsSync(lockPath));
});

test("in a terminal a tick fills a slot whose review ended, and never starts a second review of a pull request under review", async () => {
  const dir = join(tmp(), "r10");
  const first = held();
  const runs = { 7: [first.done, Promise.resolve(0)], 8: [Promise.resolve(0)], 9: [Promise.resolve(0)] };
  let prs = [pr(), pr({ number: 8, headRefOid: "c".repeat(40) })];
  let naps = 0;
  const got = await drive(["--checkout", dir], {
    list: () => ({ ok: true, status: 0, out: JSON.stringify(prs), err: "" }),
    start: (n) => runs[n].shift(),
    sleep: async () => {
      naps++;
      // While #7's review runs, #7 gets a new head and #9 arrives; #8's review has ended.
      if (naps === 1) prs = [pr({ headRefOid: "e".repeat(40) }), pr({ number: 9, headRefOid: "d".repeat(40) })];
      if (naps === 2) {
        prs = [pr({ headRefOid: "e".repeat(40) })];
        first.end(1);
      }
      if (naps === 3) throw new Error("stop");
      await new Promise((r) => setImmediate(r));
    },
  });
  assert.equal(got.code, 2);
  // Tick 1: #7 and #8. Tick 2: #8's slot goes to #9; #7, still under review, waits. Tick 3: #7's new head.
  assert.deepEqual(got.reviews.map((r) => [r.n, r.head.slice(0, 1), r.dir]), [["7", "b", dir], ["8", "c", `${dir}-2`], ["9", "d", `${dir}-2`], ["7", "e", dir]]);
});

test("a head whose checkout cannot be prepared gives its slot to the next head", async () => {
  // #7's checkout cannot be prepared; #8 and #9 take the two slots and #10 waits.
  const prs = [pr(), pr({ number: 8, headRefOid: "c".repeat(40) }), pr({ number: 9, headRefOid: "d".repeat(40) }), pr({ number: 10, headRefOid: "e".repeat(40) })];
  const got = await drive(["--once", "--checkout", join(tmp(), "r2")], {
    prs,
    git: (args) => (args[0] === "fetch" && args.includes("pull/7/head") ? { ok: false, err: "network down" } : {}),
    exit: 2,
  });
  assert.equal(got.code, 0, got.stderr);
  assert.deepEqual(got.reviews.map((r) => r.n), ["8", "9", "10"], "#10 takes a slot on the next tick");
  assert.ok(got.logs.includes("#8 at ccccccc: the review exited 2"), "a review that fails is reported with its exit code");
  // #7 tries both slots before the next head goes.
  assert.match(got.logs.join("\n"), /^#7 at bbbbbbb: reviewing; #8, #9, #10 next\n(#7 at bbbbbbb: git fetch -q failed: network down\n){2}#8 at ccccccc: reviewing; #9, #10 next\n#9 at ddddddd: reviewing; #10 next\n#10 wait for a free slot\n/);
});

test("a head whose slot's checkout cannot be prepared is reviewed in the other slot", async () => {
  // The first slot's checkout is stuck (a lock file a killed git left), whatever the head.
  const dir = join(tmp(), "r12");
  const got = await drive(["--once", "--checkout", dir], {
    prs: [pr(), pr({ number: 8, headRefOid: "c".repeat(40) })],
    git: (args, cwd) => (args[0] === "reset" && cwd === dir ? { ok: false, err: "index.lock exists" } : {}),
  });
  assert.equal(got.code, 0, got.stderr);
  // #8 waits while #7 holds the one slot that works, then takes it.
  assert.deepEqual(got.reviews.map((r) => [r.n, r.dir]), [["7", `${dir}-2`], ["8", `${dir}-2`]]);
});

test("heads that cannot start use up the task's window, and in a terminal an hour of a tick", async () => {
  // Each failing prepare takes 25 minutes of the clock, and each head tries both slots.
  let t = NOW;
  const prs = [pr(), pr({ number: 8, headRefOid: "c".repeat(40) }), pr({ number: 9, headRefOid: "d".repeat(40) }), pr({ number: 10, headRefOid: "e".repeat(40) })];
  const got = await drive(["--once", "--checkout", join(tmp(), "r7")], {
    prs,
    git: (args) => (args[0] === "fetch" ? ((t += 25 * 60 * 1000), { ok: false, err: "timed out" }) : {}),
    now: () => t,
  });
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviews.length, 0);
  assert.equal(got.calls.filter((c) => c.cmd === "git" && c.args[0] === "fetch").length, 2, "the second head finds the window gone");
  assert.match(got.logs.at(-1), /^out of time; #8, #9, #10 wait for the next run$/);
  // A terminal's tick has an hour, counted from the tick's start: the third head finds it gone.
  t = NOW;
  const loop = await drive(["--checkout", join(tmp(), "r7b")], {
    prs,
    git: (args) => (args[0] === "fetch" ? ((t += 25 * 60 * 1000), { ok: false, err: "timed out" }) : {}),
    now: () => t,
    sleep: async () => {
      throw new Error("stop");
    },
  });
  assert.equal(loop.calls.filter((c) => c.cmd === "git" && c.args[0] === "fetch").length, 4);
  assert.ok(loop.logs.includes("out of time; #9, #10 wait for the next run"));
});

test("a tool that is down on a later tick of the task's run is logged, and the run fills its slots again", async () => {
  const first = held();
  const prs = [pr()];
  let lists = 0;
  let naps = 0;
  const got = await drive(["--once", "--checkout", join(tmp(), "r15")], {
    prs,
    list: () => (++lists === 2 ? { ok: false, status: 1, out: "", err: "gh: connection reset" } : null),
    start: (n) => (n === 7 ? first.done : Promise.resolve(0)),
    sleep: async () => {
      if (++naps === 2) prs.push(pr({ number: 8, headRefOid: "c".repeat(40) }));
      if (naps === 3) first.end(0);
      await settle();
    },
  });
  assert.equal(got.code, 0, got.stderr);
  assert.ok(got.logs.includes("gh pr list failed: gh: connection reset"));
  assert.deepEqual(got.reviews.map((r) => r.n), ["7", "8"]);
});

test("auto-merge on an unreviewed head is disabled before anything else", async () => {
  const got = await drive(["--once", "--checkout", join(tmp(), "r3")], { prs: [pr({ autoMergeRequest: { enabledAt: "x" }, statusCheckRollup: [ci({ status: "IN_PROGRESS", conclusion: "" })] })] });
  assert.deepEqual(got.gh.slice(1), ["pr merge 7 --disable-auto"]);
  assert.equal(got.reviews.length, 0);
  assert.match(got.logs[0], /#7: auto-merge was on for an unreviewed or held head; disabled it/);
});

test("two overlapping ticks: the second finds the lock held and does nothing", async () => {
  const path = join(tmp(), "shared-lock");
  writeFileSync(path, "4242");
  const got = await drive(["--once", "--checkout", join(tmp(), "r4")], { lockPath: path, alive: (pid) => pid === 4242 });
  assert.equal(got.code, 0);
  assert.match(got.stdout, /another tick holds .*shared-lock; nothing to do/);
  assert.equal(got.calls.length, 0, "no gh, git or review ran");
  assert.equal(readFileSync(path, "utf8"), "4242", "the other tick's lock is untouched");
});

test("without --once the poller ticks, waits five minutes, and ticks again, through a tool that is down for one tick; stopped, it releases the lock", async () => {
  const waits = [];
  const stop = new Error("stop");
  let lists = 0;
  const got = await drive(["--checkout", join(tmp(), "r5")], {
    // gh is down on the second tick only.
    list: () => (++lists === 2 ? { ok: false, status: 1, out: "", err: "gh: connection reset" } : null),
    sleep: async (ms) => {
      waits.push(ms);
      if (waits.length === 3) throw stop;
    },
  });
  assert.equal(got.code, 2);
  assert.deepEqual(waits, [INTERVAL_MS, INTERVAL_MS, INTERVAL_MS]);
  assert.equal(got.reviews.length, 1, "a review on the first tick, none on the tick gh was down, and none of a head already reviewed");
  assert.match(got.logs.join("\n"), /gh pr list failed: gh: connection reset/);
  assert.ok(!existsSync(got.lockPath));
  assert.equal(INTERVAL_MS, 5 * 60 * 1000);
});

test("a loop whose lock was taken over stops rather than run beside the new tick, and leaves that tick's lock alone", async () => {
  const path = join(tmp(), "taken-lock");
  let naps = 0;
  const got = await drive(["--checkout", join(tmp(), "r8")], {
    lockPath: path,
    // Another tick took the lock over while this loop slept; a loop that sleeps again did not notice.
    sleep: async () => {
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

test("with --once a tool that is down ends the tick with exit 2", async () => {
  const got = await drive(["--once", "--checkout", join(tmp(), "r6")], { list: () => ({ ok: false, status: 1, out: "", err: "gh: connection reset" }) });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /gh pr list failed: gh: connection reset/);
  assert.ok(!existsSync(got.lockPath));
});

test("a usage error exits 2 before anything runs", async () => {
  for (const argv of [["--twice"], ["--checkout"], ["--once", "extra"]]) {
    const got = await drive(argv);
    assert.equal(got.code, 2, argv.join(" "));
    assert.match(got.stderr, /usage/);
    assert.equal(got.calls.length, 0);
  }
});

test("a review that cannot start frees its slot and the lock like one that failed", async () => {
  const got = await drive(["--once", "--checkout", join(tmp(), "r13")], {
    start: () => {
      throw new Error("spawn EINVAL");
    },
  });
  assert.equal(got.code, 0, got.stderr);
  assert.ok(got.logs.includes("#7 at bbbbbbb: the review exited spawn EINVAL"));
  assert.ok(!existsSync(got.lockPath));
});

test("a terminal loop that stops keeps the lock until the reviews it started have ended", async () => {
  const review = held();
  const lockPath = join(tmp(), "stop-lock");
  const run = drive(["--checkout", join(tmp(), "r14")], {
    start: () => review.done,
    lockPath,
    sleep: async () => {
      throw new Error("stop");
    },
  });
  await settle();
  assert.equal(await Promise.race([run, Promise.resolve("running")]), "running");
  assert.ok(existsSync(lockPath), "no other tick takes up the head still under review");
  review.end(0);
  const got = await run;
  assert.equal(got.code, 2);
  assert.ok(!existsSync(lockPath));
});

test("the task's window counts from the run's start, not the tick's", async () => {
  // #7's review runs throughout. At 25 minutes #8 and #9 arrive; #8's checkout
  // takes 25 minutes to fail, so #9 comes up at 50 minutes, past the window.
  let t = NOW;
  let naps = 0;
  const first = held();
  const prs = [pr()];
  const got = await drive(["--once", "--checkout", join(tmp(), "r16")], {
    prs,
    git: (args) => (args[0] === "fetch" && args.includes("pull/8/head") ? ((t += 25 * 60 * 1000), { ok: false, err: "timed out" }) : {}),
    start: (n) => (n === 7 ? first.done : Promise.resolve(0)),
    now: () => t,
    sleep: async () => {
      if (++naps === 1) {
        t += 25 * 60 * 1000;
        prs.push(pr({ number: 8, headRefOid: "c".repeat(40) }), pr({ number: 9, headRefOid: "d".repeat(40) }));
      }
      if (naps === 2) first.end(0);
      await settle();
    },
  });
  assert.equal(got.code, 0, got.stderr);
  assert.deepEqual(got.reviews.map((r) => r.n), ["7"]);
  assert.ok(got.logs.includes("out of time; #9 wait for the next run"));
});

// The real review launcher, on a stand-in review script in a scratch checkout.
function stand(body) {
  const dir = mkdtempSync(join(tmp(), "launch-"));
  mkdirSync(join(dir, "scripts"));
  writeFileSync(join(dir, "scripts", "merge-review.mjs"), body);
  return dir;
}

test("a review's output reaches the log line by line, marked and without control characters, and its exit code ends it", async () => {
  const dir = stand('console.log("one\\x1b[2Jtwo"); console.error("three"); process.exit(3);');
  const logs = [];
  assert.equal(await startReview(5, HEAD, dir, (l) => logs.push(l)), 3);
  assert.deepEqual(logs.sort(), ["#5 | one[2Jtwo", "#5 | three"]);
});

test("a review that runs past its limit is ended, with what it started", async () => {
  const dir = stand(
    [
      'import { spawn } from "node:child_process";',
      'import { writeFileSync } from "node:fs";',
      'const kid = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });',
      'writeFileSync("pids", process.pid + " " + kid.pid);',
      "setInterval(() => {}, 1000);",
    ].join("\n")
  );
  const status = await startReview(5, HEAD, dir, () => {}, { limit: 1500 });
  assert.equal(status, ENDED);
  await new Promise((r) => setTimeout(r, 500));
  for (const pid of readFileSync(join(dir, "pids"), "utf8").split(" ").map(Number)) {
    assert.throws(() => process.kill(pid, 0), /ESRCH/, `process ${pid} still runs`);
  }
});


test("a review whose leftover process holds its output open still ends when it exits", async () => {
  // A stand-in process that exits but whose output never ends, as when a
  // process the review started still holds the pipes.
  const child = new EventEmitter();
  child.pid = 0;
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  const launch = () => {
    setImmediate(() => {
      child.stdout.write("done\n");
      child.emit("exit", 0, null);
    });
    return child;
  };
  const logs = [];
  const status = await Promise.race([startReview(5, HEAD, "unused", (l) => logs.push(l), { limit: 60_000, launch }), new Promise((r) => setTimeout(() => r("still waiting"), 5000))]);
  assert.equal(status, 0, "the exit ends it, without waiting for the output to end");
  assert.deepEqual(logs, ["#5 | done"]);
});

test("a review ended at its limit marks its head's merge review as failed, so the head is not taken up again", async () => {
  const got = await drive(["--once", "--checkout", join(tmp(), "r17")], { start: () => Promise.resolve(ENDED) });
  assert.equal(got.code, 0, got.stderr);
  const mark = got.gh.find((c) => c.startsWith("api "));
  assert.ok(mark, "the status is set");
  assert.match(mark, new RegExp(`^api repos/\\{owner\\}/\\{repo\\}/statuses/${HEAD} -f state=error -f context=Merge review -f description=The review ran past 70 minutes and was ended`));
  // A review that ends on its own sets its own status.
  const own = await drive(["--once", "--checkout", join(tmp(), "r18")], { exit: 1 });
  assert.ok(!own.gh.some((c) => c.startsWith("api ")));
});
