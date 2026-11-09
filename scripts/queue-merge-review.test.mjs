// scripts/queue-merge-review.mjs: what the merge queue's "Merge review" check decides.
import assert from "node:assert/strict";
import { test } from "node:test";

import { main, pullRequestOf, reviewState } from "./queue-merge-review.mjs";

const HEAD = "a".repeat(40);
const REF = "refs/heads/gh-readonly-queue/main/pr-160-0123456789abcdef";
const REPO = "o/r";
const review = (state) => ({ context: "Merge review", state });

// A stand-in for gh: answers each path from `answers`, and records the paths.
function gh(answers) {
  const paths = [];
  const run = (cmd, args) => {
    assert.equal(cmd, "gh");
    const path = args[1];
    paths.push(path);
    const answer = answers[path];
    if (answer === undefined) return { ok: false, out: "", err: `HTTP 404: ${path}` };
    return { ok: true, out: typeof answer === "string" ? answer : JSON.stringify(answer), err: "" };
  };
  return { run, paths };
}

const pull = { [`repos/${REPO}/pulls/160`]: { head: { sha: HEAD } } };
const withStatuses = (list) => ({ ...pull, [`repos/${REPO}/commits/${HEAD}/statuses?per_page=100`]: list });
const decide = (answers, ref = REF) => main({ ref, repo: REPO }, { run: gh(answers).run });

test("a head with a successful review passes", () => {
  const r = decide(withStatuses([review("success")]));
  assert.equal(r.code, 0);
  assert.match(r.stdout, /#160 at aaaaaaa passed/);
});

test("the newest status of the review's name decides, whatever else the head carries", () => {
  assert.equal(decide(withStatuses([{ context: "Other", state: "failure" }, review("success"), review("failure")])).code, 0);
  assert.equal(decide(withStatuses([review("failure"), review("success")])).code, 1);
});

test("a head with no review fails and says so", () => {
  const r = decide(withStatuses([{ context: "Other", state: "success" }]));
  assert.equal(r.code, 1);
  assert.match(r.stderr, /has no merge review/);
});

for (const [state, why] of [
  ["failure", /blocks it/],
  ["error", /could not finish/],
  ["pending", /still running/],
]) {
  test(`a ${state} review fails and says why`, () => {
    const r = decide(withStatuses([review(state)]));
    assert.equal(r.code, 1);
    assert.match(r.stderr, why);
  });
}

test("a review on an older head does not count: only the current head's statuses are read", () => {
  const older = "b".repeat(40);
  const answers = { ...withStatuses([]), [`repos/${REPO}/commits/${older}/statuses?per_page=100`]: [review("success")] };
  const { run, paths } = gh(answers);
  const r = main({ ref: REF, repo: REPO }, { run });
  assert.equal(r.code, 1);
  assert.match(r.stderr, /has no merge review/);
  assert.ok(!paths.some((p) => p.includes(older)));
});

test("a queue entry whose pull request cannot be found fails", () => {
  const r = decide({});
  assert.equal(r.code, 1);
  assert.match(r.stderr, /Could not read pull request #160/);
});

test("a queue ref that names no pull request fails before asking GitHub", () => {
  for (const ref of ["refs/heads/main", "", undefined]) {
    const { run, paths } = gh({});
    assert.equal(main({ ref, repo: REPO }, { run }).code, 1);
    assert.deepEqual(paths, []);
  }
});

test("statuses that cannot be read or parsed fail rather than pass", () => {
  assert.equal(decide(pull).code, 1);
  assert.equal(decide(withStatuses("not json")).code, 1);
  assert.equal(decide(withStatuses({ message: "nope" })).code, 1);
});

test("the pull request is the number in the queue's branch name", () => {
  assert.equal(pullRequestOf(REF), "160");
  assert.equal(pullRequestOf("refs/heads/feature/pr-160"), null);
  assert.equal(reviewState([]), null);
});
