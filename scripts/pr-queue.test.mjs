// Tests for pr-queue.mjs's classification, run without GitHub.
// Run: node --test scripts/pr-queue.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { changes, classify, fingerprint, markShared, signature } from "./pr-queue.mjs";

const job = "Build, lint, test, dependencies (Linux)\tTest\t2026-11-03T06:41:47.5718425Z ";

test("a signature names the failed tests", () => {
  const log = [
    `${job}test a_passing_one ... ok`,
    `${job}test the_replay_server_sends_notifications ... FAILED`,
    `${job}##[error]Process completed with exit code 101.`,
  ].join("\n");
  assert.equal(signature(log), "tests failed: the_replay_server_sends_notifications");
});

test("a signature skips the exit-code line for the error above it", () => {
  const log = [`${job}line 1: node: command not found`, `${job}##[error]Process completed with exit code 127.`].join("\n");
  assert.equal(signature(log), "line 1: node: command not found");
});

test("a signature keeps an error line that says what failed", () => {
  assert.equal(signature(`${job}##[error]The runner has lost communication with the server`), "The runner has lost communication with the server");
});

const check = (over) => ({ name: "CI", status: "COMPLETED", conclusion: "SUCCESS", ...over });
const pr = (over) => ({ isDraft: false, mergeable: "MERGEABLE", statusCheckRollup: [check()], ...over });
const none = { failures: {}, containsMain: true };

test("states, first match wins", () => {
  assert.deepEqual(classify(pr({ isDraft: true, mergeable: "CONFLICTING" }), none), { state: "draft" });
  assert.deepEqual(classify(pr({ mergeable: "CONFLICTING" }), none), { state: "conflicting" });
  assert.deepEqual(classify(pr({ statusCheckRollup: [check({ status: "IN_PROGRESS", conclusion: "" })] }), none), { state: "ci-running" });
  assert.deepEqual(
    classify(pr({ statusCheckRollup: [check({ conclusion: "FAILURE" })] }), { failures: { CI: "node: command not found" }, containsMain: true }),
    { state: "ci-red", signature: "node: command not found" }
  );
  assert.deepEqual(classify(pr(), { failures: {}, containsMain: false }), { state: "behind-main" });
  assert.deepEqual(classify(pr(), none), { state: "green" });
});

test("a skipped check is not a failure", () => {
  assert.deepEqual(classify(pr({ statusCheckRollup: [check(), check({ name: "Windows", conclusion: "SKIPPED" })] }), none), { state: "green" });
});

test("a signature two pull requests share is marked shared", () => {
  const rows = markShared([
    { number: 1, state: "ci-red", signature: "runner" },
    { number: 2, state: "ci-red", signature: "runner" },
    { number: 3, state: "ci-red", signature: "tests failed: x" },
  ]);
  assert.deepEqual(rows.map((r) => !!r.shared), [true, true, false]);
});

test("the fingerprint ignores order and titles, and follows head, state and signature", () => {
  const a = [{ number: 1, head: "aaa", state: "green", title: "x" }, { number: 2, head: "bbb", state: "ci-red", signature: "s" }];
  const b = [{ ...a[1], title: "renamed" }, a[0]];
  assert.equal(fingerprint(a), fingerprint(b));
  assert.notEqual(fingerprint(a), fingerprint([a[0], { ...a[1], signature: "t" }]));
  assert.notEqual(fingerprint(a), fingerprint([a[0], { ...a[1], head: "ccc" }]));
  assert.notEqual(fingerprint(a), fingerprint([a[0], { ...a[1], feedback: 1 }]));
});

test("new comments are a change even when the state is not", () => {
  const before = [{ number: 1, head: "a", state: "green", feedback: 2 }];
  assert.deepEqual(changes(before, [{ ...before[0], feedback: 3 }]), [{ number: 1, change: "new comments", from: "green", to: "green" }]);
  assert.deepEqual(changes(before, [{ ...before[0] }]), []);
});

test("changes lists new, moved and gone pull requests", () => {
  const before = [{ number: 1, head: "a", state: "ci-red", signature: "s" }, { number: 2, head: "b", state: "green" }];
  const after = [{ number: 1, head: "a", state: "green" }, { number: 3, head: "c", state: "ci-running" }];
  assert.deepEqual(changes(before, after), [
    { number: 1, change: "state", from: "ci-red", to: "green" },
    { number: 2, change: "closed or merged", from: "green" },
    { number: 3, change: "new", to: "ci-running" },
  ]);
});

test("a signature takes the error line above the exit code, not the last line", () => {
  const log = [
    `${job}##[endgroup]`,
    `${job}npm error code ENOENT`,
    `${job}npm error You can rerun the command with --loglevel=verbose`,
    `${job}##[error]Process completed with exit code 1.`,
  ].join("\n");
  assert.equal(signature(log), "npm error code ENOENT");
});
