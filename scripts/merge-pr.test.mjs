// Tests for merge-pr.mjs's gate, run without GitHub.
// Run: node --test scripts/merge-pr.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { refusals } from "./merge-pr.mjs";

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
