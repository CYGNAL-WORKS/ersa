// Tests for shepherd.mjs's decisions, run without GitHub or Claude Code.
// Run: node --test scripts/shepherd.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { NEVER_STARTED } from "./pr-queue.mjs";
import { ciTask, decide, readiness } from "./shepherd.mjs";

const opts = { merge: true, maxFixes: 2 };
const facts = (over = {}) => ({
  dependabot: false, reviewHere: null, fixHere: false, fixes: 0, failed: { review: 0, fix: 0 },
  mainMergedHere: false, rerunHere: false, ready: null, ...over,
});
const row = (state, over = {}) => ({ number: 1, head: "aaaaaaa", state, ...over });
const kind = (r, f = facts(), o = opts) => decide(r, f, o).kind;

test("the gate's dry run maps to readiness", () => {
  assert.equal(readiness(0, "#1 may merge at abc"), "merge");
  assert.equal(readiness(1, "- abc, after the review, is not a merge; review the head"), "review");
  assert.equal(readiness(1, "- its head does not contain main's tip; merge main in and let CI run"), "behind");
  assert.equal(readiness(1, "- its head does not contain main's tip; merge main in\n- abc, after the review, is not a merge; review the head"), "review");
  assert.equal(readiness(1, "- GitHub reports it unknown"), "wait");
  assert.equal(readiness(2, "gh pr view failed"), "wait");
});

test("drafts and running CI wait; a dependency update is the owner's", () => {
  assert.equal(kind(row("draft")), "wait");
  assert.equal(kind(row("ci-running")), "wait");
  assert.equal(kind(row("green"), facts({ dependabot: true })), "owner");
});

test("a green pull request with no review gets one, then a fix for findings", () => {
  assert.equal(kind(row("green")), "review");
  assert.equal(kind(row("behind-main")), "review");
  const findings = facts({ reviewHere: { verdict: "findings" } });
  assert.deepEqual(decide(row("green"), findings, opts), { kind: "fix", task: "review" });
  assert.equal(kind(row("green"), { ...findings, fixHere: true }), "owner");
  assert.equal(kind(row("green"), { ...findings, fixes: 2 }), "owner");
  assert.equal(kind(row("green"), { ...findings, failed: { review: 0, fix: 2 } }), "owner");
});

test("a review job that failed twice on a head goes to the owner", () => {
  assert.equal(kind(row("green"), facts({ failed: { review: 1, fix: 0 } })), "review");
  assert.equal(kind(row("green"), facts({ failed: { review: 2, fix: 0 } })), "owner");
});

test("a clean review merges through the gate, or waits on the owner without --merge", () => {
  assert.equal(kind(row("green"), facts({ ready: "merge" })), "merge");
  assert.equal(kind(row("green"), facts({ ready: "merge" }), { ...opts, merge: false }), "owner");
  assert.equal(kind(row("green"), facts({ ready: "wait" })), "wait");
});

test("a reviewed pull request behind main gets main merged in once per head", () => {
  assert.equal(kind(row("behind-main"), facts({ ready: "behind" })), "merge-main");
  assert.equal(kind(row("behind-main"), facts({ ready: "behind", mainMergedHere: true })), "owner");
});

test("a head the reviewer never saw is reviewed again", () => {
  assert.equal(kind(row("green"), facts({ ready: "review" })), "review");
});

test("a conflict merges main in, then goes to a fix", () => {
  assert.equal(kind(row("conflicting")), "merge-main");
  assert.deepEqual(decide(row("conflicting"), facts({ mainMergedHere: true }), opts), { kind: "fix", task: "conflict" });
  assert.equal(kind(row("conflicting"), facts({ mainMergedHere: true, fixes: 2 })), "owner");
});

test("red CI reruns once per head; then a shared failure is the owner's and one of its own gets a fix", () => {
  assert.equal(kind(row("ci-red", { signature: NEVER_STARTED })), "rerun");
  assert.equal(kind(row("ci-red", { signature: "tests failed: a" })), "rerun");
  assert.equal(kind(row("ci-red", { signature: "x", shared: true }), facts({ rerunHere: true })), "owner");
  assert.deepEqual(decide(row("ci-red", { signature: "tests failed: a" }), facts({ rerunHere: true }), opts), { kind: "fix", task: "ci" });
  assert.equal(kind(row("ci-red", { signature: "tests failed: a" }), facts({ rerunHere: true, fixes: 2 })), "owner");
});

test("a CI task names the signature and where the log is", () => {
  const t = ciTask("118", "tests failed: a");
  assert.match(t, /CI failed on this head: tests failed: a\./);
  assert.match(t, /gh pr checks 118/);
  assert.match(t, /change nothing and say so/);
});
