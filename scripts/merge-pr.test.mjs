// Tests for merge-pr.mjs's gate, run without GitHub.
// Run: node --test scripts/merge-pr.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { commitsSinceReview, refusals } from "./merge-pr.mjs";

const check = (over) => ({ name: "CI", status: "COMPLETED", conclusion: "SUCCESS", startedAt: "2026-11-03T10:00:00Z", ...over });
const facts = (over = {}) => ({
  containsMain: true,
  reviewedInBranch: true,
  sinceReview: [],
  ...over,
  pr: { state: "OPEN", isDraft: false, mergeable: "MERGEABLE", statusCheckRollup: [check()], ...over.pr },
});
const mergeOfMain = { sha: "aaaaaaa1", merge: true, fromMain: true, clean: true };

test("a reviewed head with green CI that contains main may merge", () => {
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
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ status: "IN_PROGRESS", conclusion: "" })] } })), ["CI is still running: CI"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ conclusion: "FAILURE" })] } })), ["CI did not pass: CI"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [check({ conclusion: "CANCELLED" })] } })), ["CI did not pass: CI"]);
});

test("only the latest run of a check counts, and a skipped check passes", () => {
  const rerun = [check({ conclusion: "FAILURE" }), check({ startedAt: "2026-11-03T11:00:00Z" })];
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [...rerun, check({ name: "Windows", conclusion: "SKIPPED" })] } })), []);
});

test("a commit status counts as a check", () => {
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [{ context: "ext", state: "SUCCESS" }] } })), []);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [{ context: "ext", state: "PENDING" }] } })), ["CI is still running: ext"]);
  assert.deepEqual(refusals(facts({ pr: { statusCheckRollup: [{ context: "ext", state: "ERROR" }] } })), ["CI did not pass: ext"]);
});

test("a head behind main may not merge", () => {
  assert.deepEqual(refusals(facts({ containsMain: false })), ["its head does not contain main's tip; merge main in and let CI run"]);
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
// branch, and a plain commit.
function repo() {
  const dir = mkdtempSync(join(tmpdir(), "merge-pr-"));
  const g = (...args) => {
    const r = spawnSync("git", args, { cwd: dir, encoding: "utf8" });
    if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr}`);
    return r.stdout.trim();
  };
  const commit = (file, text, msg) => {
    writeFileSync(join(dir, file), text);
    g("add", file);
    g("commit", "-q", "-m", msg);
    return g("rev-parse", "HEAD");
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
  commit("m1.txt", "m1\n", "main 1");
  g("switch", "-q", "pr");
  g("merge", "-q", "--no-edit", "main");
  const clean = g("rev-parse", "HEAD");
  g("switch", "-q", "main");
  commit("a.txt", "main\n", "main 2, conflicting");
  g("switch", "-q", "pr");
  spawnSync("git", ["merge", "-q", "--no-edit", "main"], { cwd: dir });
  writeFileSync(join(dir, "a.txt"), "resolved by hand\n");
  g("add", "a.txt");
  g("commit", "-q", "--no-edit");
  const conflicted = g("rev-parse", "HEAD");
  g("merge", "-q", "--no-edit", "side");
  const other = g("rev-parse", "HEAD");
  const plain = commit("p.txt", "p\n", "after review");
  const git = (args) => {
    const r = spawnSync("git", args, { cwd: dir, encoding: "utf8" });
    return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: (r.stderr ?? "").trim() };
  };
  return { dir, git, side, reviewed, clean, conflicted, other, plain };
}

test("the commits after the review are read and classified from git", () => {
  const r = repo();
  try {
    const got = commitsSinceReview({ reviewed: r.reviewed, head: r.plain, main: "main", git: r.git });
    const by = Object.fromEntries(got.map((c) => [c.sha, c]));
    // The other branch's own commit came in unreviewed too.
    assert.equal(got.length, 5);
    assert.deepEqual(by[r.side], { sha: r.side, merge: false, fromMain: false, clean: false });
    assert.deepEqual(by[r.clean], { sha: r.clean, merge: true, fromMain: true, clean: true });
    assert.deepEqual(by[r.conflicted], { sha: r.conflicted, merge: true, fromMain: true, clean: false });
    assert.deepEqual(by[r.other], { sha: r.other, merge: true, fromMain: false, clean: true });
    assert.deepEqual(by[r.plain], { sha: r.plain, merge: false, fromMain: false, clean: false });
    assert.deepEqual(commitsSinceReview({ reviewed: r.reviewed, head: r.clean, main: "main", git: r.git }), [
      { sha: r.clean, merge: true, fromMain: true, clean: true },
    ]);
  } finally {
    rmSync(r.dir, { recursive: true, force: true });
  }
});

test("a failed git command throws rather than reading as no commits or a clean merge", () => {
  const failing = (which) => (args) =>
    args[0] === which ? { ok: false, out: "", err: "boom" } : args[0] === "rev-list" ? { ok: true, out: "m1 p1 p2", err: "" } : { ok: true, out: "", err: "" };
  assert.throws(() => commitsSinceReview({ reviewed: "r", head: "h", git: failing("rev-list") }), /rev-list.*boom/);
  assert.throws(() => commitsSinceReview({ reviewed: "r", head: "h", git: failing("show") }), /show.*boom/);
});
