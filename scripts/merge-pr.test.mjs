// Tests for merge-pr.mjs's gate, run without GitHub.
// Run: node --test scripts/merge-pr.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";

import { ciRefusals, commitsSinceReview, latestChecks, main, refusals } from "./merge-pr.mjs";

const check = (over) => ({ name: "CI", status: "COMPLETED", conclusion: "SUCCESS", startedAt: "2026-11-03T10:00:00Z", ...over });
const facts = (over = {}) => ({
  reviewedInBranch: true,
  sinceReview: [],
  ...over,
  pr: { state: "OPEN", isDraft: false, mergeable: "MERGEABLE", statusCheckRollup: [check()], ...over.pr },
});
const mergeOfMain = { sha: "aaaaaaa1", merge: true, fromMain: true, clean: true };

test("a reviewed head with green CI may merge", () => {
  assert.deepEqual(refusals(facts()), []);
});

test("clean merges of main after the review need no new review", () => {
  assert.deepEqual(refusals(facts({ sinceReview: [mergeOfMain, { ...mergeOfMain, sha: "bbbbbbb2" }] })), []);
});

test("a closed, draft or conflicting pull request may not merge", () => {
  assert.deepEqual(refusals(facts({ pr: { state: "MERGED" } })), ["it is merged"]);
  assert.deepEqual(refusals(facts({ pr: { isDraft: true } })), ["it is a draft"]);
  assert.deepEqual(refusals(facts({ pr: { mergeable: "CONFLICTING" } })), ["GitHub reports it conflicting"]);
  assert.deepEqual(refusals(facts({ pr: { mergeable: "UNKNOWN" } })), ["GitHub reports it unknown"]);
});

test("CI must have run, finished and passed", () => {
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [] } })), ["it has no CI checks"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ name: "Merge review" })] } })), ["it has no CI checks"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ name: "Merge review" }), check()] } })), []);
  assert.deepEqual(ciRefusals([check()]), []);
  assert.deepEqual(ciRefusals([check(), { context: "Merge review", state: "FAILURE" }]), [], "the review's own status is not CI");
  assert.deepEqual(ciRefusals(undefined), ["it has no CI checks"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ status: "IN_PROGRESS", conclusion: "" })] } })), ["CI is still running: CI"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ conclusion: "FAILURE" })] } })), ["CI did not pass: CI"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ conclusion: "CANCELLED" })] } })), ["CI did not pass: CI"]);
});

test("latestChecks keeps the newest run of each check by name", () => {
  const checks = [check({ conclusion: "FAILURE" }), check({ startedAt: "2026-11-03T11:00:00Z" }), check({ name: "Lint", startedAt: undefined })];
  assert.deepEqual(latestChecks(checks), [checks[1], checks[2]]);
  // The order GitHub lists them in does not matter.
  assert.deepEqual(latestChecks([checks[1], checks[0]]), [checks[1]]);
  assert.deepEqual(latestChecks([]), []);
});

test("only the latest run of a check counts, and a skipped or neutral check passes", () => {
  const rerun = [check({ conclusion: "FAILURE" }), check({ startedAt: "2026-11-03T11:00:00Z" })];
  const others = [check({ name: "Windows", conclusion: "SKIPPED" }), check({ name: "Lint", conclusion: "NEUTRAL" })];
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [...rerun, ...others] } })), []);
});

test("a commit status counts as a check", () => {
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [{ context: "ext", state: "SUCCESS" }] } })), []);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [{ context: "ext", state: "PENDING" }] } })), ["CI is still running: ext"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [{ context: "ext", state: "ERROR" }] } })), ["CI did not pass: ext"]);
});

test("a red merge review blocks the merge; a pending or green one is not CI the gate waits for", () => {
  const review = (state) => ({ context: "Merge review", state });
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check(), review("FAILURE")] } })), ["the merge review blocks the merge; see the review on the pull request"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check(), review("ERROR")] } })), ["the merge review blocks the merge; see the review on the pull request"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check(), review("PENDING")] } })), []);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check(), review("SUCCESS")] } })), []);
});

test("code no reviewer saw sends the head back for review", () => {
  assert.deepEqual(refusals(facts({ reviewedInBranch: false })), ["the reviewed commit is not in the branch's history; review the head"]);
  assert.deepEqual(refusals(facts({ sinceReview: [{ ...mergeOfMain, merge: false, fromMain: false, clean: false }] })), [
    "aaaaaaa, after the review, is not a merge; review the head",
  ]);
  assert.deepEqual(refusals(facts({ sinceReview: [{ ...mergeOfMain, fromMain: false }] })), ["aaaaaaa, after the review, merges a branch other than main; review the head"]);
  assert.deepEqual(refusals(facts({ sinceReview: [{ ...mergeOfMain, clean: false }] })), ["aaaaaaa, after the review, resolved a conflict by hand; review the head"]);
});

test("an expected commit status counts as still running", () => {
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [{ context: "ext", state: "EXPECTED" }] } })), ["CI is still running: ext"]);
});

// A real repository: main and a branch, a reviewed commit, then a clean merge
// of main, a merge of main that resolved a conflict by hand, a merge of another
// branch, a merge of main that resolved a conflict by keeping the branch's side,
// an octopus merge of two main commits, an octopus merge of main and the other
// branch, and a plain commit. git reads no global or system config and no GIT_*
// variables, so the fixture is the same on every machine.
// Built once and shared: building it takes about two seconds on Windows.
let fixture;
const shared = () => (fixture ??= repo());
after(() => fixture && rmSync(fixture.home, { recursive: true, force: true }));

function repo() {
  const home = mkdtempSync(join(tmpdir(), "merge-pr-"));
  const dir = join(home, "repo");
  mkdirSync(dir);
  writeFileSync(join(home, "gitconfig"), "");
  const env = {
    ...Object.fromEntries(Object.entries(process.env).filter(([k]) => !k.toUpperCase().startsWith("GIT_"))),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_CONFIG_GLOBAL: join(home, "gitconfig"),
  };
  const git = (args) => {
    const r = spawnSync("git", args, { cwd: dir, encoding: "utf8", env });
    return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() };
  };
  try {
    const g = (...args) => {
      const r = git(args);
      if (!r.ok) throw new Error(`git ${args.join(" ")}: ${r.err}`);
      return r.out;
    };
    const commit = (file, text, msg) => {
      writeFileSync(join(dir, file), text);
      g("add", file);
      g("commit", "-q", "-m", msg);
      return g("rev-parse", "HEAD");
    };
    // A commit with exactly these parents; `git merge` would fold an octopus
    // of main commits into fewer parents.
    const octopus = (file, ...parents) => {
      writeFileSync(join(dir, file), "unreviewed\n");
      g("add", file);
      const sha = g("commit-tree", g("write-tree"), ...parents.flatMap((p) => ["-p", p]), "-m", "octopus");
      g("reset", "-q", "--hard", sha);
      return sha;
    };
    g("init", "-q", "-b", "main");
    g("config", "user.email", "t@example.com");
    g("config", "user.name", "t");
    g("config", "commit.gpgsign", "false");
    commit("a.txt", "base\n", "base");
    g("switch", "-q", "-c", "side");
    const side = commit("side.txt", "side\n", "side");
    g("switch", "-q", "main");
    g("switch", "-q", "-c", "pr");
    const reviewed = commit("a.txt", "pr\n", "pr work");
    g("switch", "-q", "main");
    const main1 = commit("m1.txt", "m1\n", "main 1");
    g("switch", "-q", "pr");
    g("merge", "-q", "--no-edit", "main");
    const clean = g("rev-parse", "HEAD");
    g("switch", "-q", "main");
    const main2 = commit("a.txt", "main\n", "main 2, conflicting");
    g("switch", "-q", "pr");
    git(["merge", "-q", "--no-edit", "main"]);
    writeFileSync(join(dir, "a.txt"), "resolved by hand\n");
    g("add", "a.txt");
    g("commit", "-q", "--no-edit");
    const conflicted = g("rev-parse", "HEAD");
    g("merge", "-q", "--no-edit", "side");
    const other = g("rev-parse", "HEAD");
    g("switch", "-q", "main");
    commit("a.txt", "main 3\n", "main 3, conflicting");
    g("switch", "-q", "pr");
    git(["merge", "-q", "--no-edit", "main"]);
    g("checkout", "--ours", "a.txt");
    g("add", "a.txt");
    g("commit", "-q", "--no-edit");
    const keptOurs = g("rev-parse", "HEAD");
    g("switch", "-q", "main");
    const main4 = commit("m4.txt", "m4\n", "main 4");
    const main5 = commit("m5.txt", "m5\n", "main 5");
    g("switch", "-q", "pr");
    const octoMain = octopus("o1.txt", "HEAD", main4, main5);
    const octoSide = octopus("o2.txt", "HEAD", main5, side);
    const plain = commit("p.txt", "p\n", "after review");
    return { home, git, side, reviewed, main1, main2, main5, clean, conflicted, other, keptOurs, octoMain, octoSide, plain };
  } catch (e) {
    rmSync(home, { recursive: true, force: true });
    throw e;
  }
}

test("the commits after the review are read and classified from git", () => {
  const r = shared();
  const got = commitsSinceReview({ reviewed: r.reviewed, head: r.plain, main: "main", git: r.git });
  const by = Object.fromEntries(got.map((c) => [c.sha, c]));
  // The other branch's own commit came in unreviewed too.
  assert.equal(got.length, 8);
  assert.deepEqual(by[r.side], { sha: r.side, merge: false, fromMain: false, clean: false });
  assert.deepEqual(by[r.clean], { sha: r.clean, merge: true, fromMain: true, clean: true });
  assert.deepEqual(by[r.conflicted], { sha: r.conflicted, merge: true, fromMain: true, clean: false });
  assert.deepEqual(by[r.other], { sha: r.other, merge: true, fromMain: false, clean: true });
  // Keeping the branch's side leaves the combined diff empty; only the
  // remerge-diff shows the resolution.
  assert.deepEqual(by[r.keptOurs], { sha: r.keptOurs, merge: true, fromMain: true, clean: false });
  // git prints no remerge-diff for an octopus merge, so one never reads as clean.
  assert.deepEqual(by[r.octoMain], { sha: r.octoMain, merge: true, fromMain: true, clean: false });
  assert.deepEqual(by[r.octoSide], { sha: r.octoSide, merge: true, fromMain: false, clean: false });
  assert.deepEqual(by[r.plain], { sha: r.plain, merge: false, fromMain: false, clean: false });
  assert.deepEqual(commitsSinceReview({ reviewed: r.reviewed, head: r.clean, main: "main", git: r.git }), [
    { sha: r.clean, merge: true, fromMain: true, clean: true },
  ]);
});

test("a failed git command throws rather than reading as no commits or a clean merge", () => {
  const failing = (which) => (args) =>
    args[0] === which ? { ok: false, out: "", err: "boom" } : args[0] === "rev-list" ? { ok: true, out: "m1 p1 p2", err: "" } : { ok: true, out: "", err: "" };
  assert.throws(() => commitsSinceReview({ reviewed: "r", head: "h", git: failing("rev-list") }), /rev-list.*boom/);
  assert.throws(() => commitsSinceReview({ reviewed: "r", head: "h", git: failing("show") }), /show.*boom/);
  // A failed merge-base reads as not from main, which refuses.
  assert.deepEqual(commitsSinceReview({ reviewed: "r", head: "h", git: failing("merge-base") }), [{ sha: "m1", merge: true, fromMain: false, clean: true }]);
});

test("an octopus merge never reads as clean, even when git prints no remerge-diff", () => {
  const git = (args) => ({ ok: true, out: args[0] === "rev-list" ? "m1 p1 p2 p3" : "", err: "" });
  assert.deepEqual(commitsSinceReview({ reviewed: "r", head: "h", git }), [{ sha: "m1", merge: true, fromMain: true, clean: false }]);
});

// main() against the real repository, with GitHub stood in for: gh answers
// from `pr` (or `view`), git fetch does nothing, and origin/main points at
// `originMain`.
function gate(r, argv, { head, originMain, pr = {}, view, merged = true, fetched = true, unknownFirst = false }) {
  r.git(["update-ref", "refs/remotes/origin/main", originMain]);
  const gh = [];
  const waits = [];
  const ok = (out = "") => ({ ok: true, out, err: "" });
  const run = (cmd, args) => {
    if (cmd === "gh") {
      gh.push(args);
      // GitHub answers UNKNOWN until it has worked the merge out; the second read has it.
      const mergeable = unknownFirst && gh.filter((a) => a[1] === "view").length === 1 ? "UNKNOWN" : "MERGEABLE";
      if (args[1] === "view") return view ?? ok(JSON.stringify({ state: "OPEN", isDraft: false, mergeable, headRefOid: head, statusCheckRollup: [check()], ...pr }));
      return merged ? ok() : { ok: false, out: "", err: "head moved" };
    }
    if (args[0] === "fetch") return fetched ? ok() : { ok: false, out: "", err: "network down" };
    return r.git(args);
  };
  return { ...main(argv, { run, wait: (ms) => waits.push(ms) }), gh, waits };
}

test("the gate adds a head that passes to the merge queue, naming the head it checked", () => {
  const r = shared();
  const at = { head: r.clean, originMain: r.main1 };
  const got = gate(r, ["5", "--reviewed", r.reviewed], at);
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.stdout, `#5 added to the merge queue at ${r.clean.slice(0, 7)}\n`);
  assert.deepEqual(got.gh.at(-1), ["pr", "merge", "5", "--auto", "--merge", "--match-head-commit", r.clean]);

  const dry = gate(r, ["--dry-run", "--reviewed", r.reviewed, "5"], at);
  assert.equal(dry.code, 0, dry.stderr);
  assert.equal(dry.stdout, `#5 may enter the merge queue at ${r.clean.slice(0, 7)}\n`);
  assert.equal(dry.gh.length, 1, "a dry run only reads the pull request");

  // A head behind main is not refused for that alone: the queue builds and tests the merge (ADR 0023).
  const behind = gate(r, ["5", "--reviewed", r.reviewed], { head: r.clean, originMain: r.main5 });
  assert.equal(behind.code, 0, behind.stderr);
  assert.equal(behind.gh.length, 2);
  assert.deepEqual(behind.waits, []);

  // A fresh head GitHub has not worked out yet is read again after a pause.
  const unknown = gate(r, ["5", "--reviewed", r.reviewed], { ...at, unknownFirst: true });
  assert.equal(unknown.code, 0, unknown.stderr);
  assert.deepEqual(unknown.waits, [5000]);
  assert.equal(unknown.gh.filter((a) => a[1] === "view").length, 2);
  const stillUnknown = gate(r, ["5", "--reviewed", r.reviewed], { ...at, pr: { mergeable: "UNKNOWN" } });
  assert.equal(stillUnknown.code, 1);
  assert.match(stillUnknown.stdout, /GitHub reports it unknown/);
  assert.deepEqual(stillUnknown.waits, [5000], "one more read, not a loop");

  const failedMerge = gate(r, ["5", "--reviewed", r.reviewed], { ...at, merged: false });
  assert.equal(failedMerge.code, 2);
  assert.match(failedMerge.stderr, /head moved/);
});

test("the gate refuses with exit 1 and never calls merge", () => {
  const r = shared();
  const refused = (argv, at, reason) => {
    const got = gate(r, argv, at);
    assert.equal(got.code, 1, got.stderr);
    assert.match(got.stdout, reason);
    assert.equal(got.gh.length, 1);
  };
  refused(["5", "--reviewed", r.reviewed], { head: r.conflicted, originMain: r.main2 }, /resolved a conflict by hand/);
  refused(["5", "--reviewed", r.clean], { head: r.reviewed, originMain: r.main1 }, /not in the branch's history/);
  refused(["5", "--reviewed", r.reviewed], { head: r.clean, originMain: r.main1, pr: { isDraft: true } }, /draft/);
});

test("a usage or tool error exits 2 before anything merges", () => {
  const r = shared();
  const at = { head: r.clean, originMain: r.main1 };
  for (const argv of [["5", "--reviewed", r.reviewed, "--reviewed", r.clean], ["5", "--reviewed", r.reviewed, "--dryrun"], ["5", "--reviewed", r.reviewed, "--dry-run=true"], ["5"], ["--reviewed", r.reviewed]]) {
    const got = gate(r, argv, at);
    assert.equal(got.code, 2, argv.join(" "));
    assert.equal(got.gh.length, 0, argv.join(" "));
  }
  const views = [
    { ok: true, out: "not json", err: "" },
    { ok: true, out: JSON.stringify({ state: null, mergeable: null, headRefOid: r.clean }), err: "" },
    { ok: false, out: "", err: "no gh" },
  ];
  for (const view of views) {
    const got = gate(r, ["5", "--reviewed", r.reviewed], { ...at, view });
    assert.equal(got.code, 2, view.out);
    assert.equal(got.gh.length, 1);
  }
  assert.equal(gate(r, ["5", "--reviewed", "nosuchcommit"], at).code, 2);
  // A failed fetch would leave origin/main stale, so the gate stops rather than judge against it.
  const offline = gate(r, ["5", "--reviewed", r.reviewed], { ...at, fetched: false });
  assert.equal(offline.code, 2);
  assert.match(offline.stderr, /git fetch failed: network down/);
  assert.equal(offline.gh.length, 1);
});
