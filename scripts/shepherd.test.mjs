// Tests for shepherd.mjs's decisions, run without GitHub or Claude Code.
// Run: node --test scripts/shepherd.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { OWNER_MERGES, blocks, ciTask, decide, fixKind, notesComment, readiness, route } from "./shepherd.mjs";

const opts = { merge: true, maxFixes: 3 };
const clean = { kind: "review", head: "r1", startedAt: "1", blocks: false, output: { findings: [] } };
const dirty = { kind: "review", head: "r1", startedAt: "1", blocks: true, output: { findings: [{ severity: "medium", origin: "new", location: "a.mjs:1", defect: "Breaks." }] } };
const facts = (over = {}) => ({
  dependabot: false, hold: false, fixHere: false, fixPending: false, fixOn: () => undefined,
  rounds: { review: 0, ci: 0, conflict: 0 }, failed: { review: 0, fix: 0 },
  mainHere: null, rerunHere: false, route: { need: "review" }, ready: null, ownerMerges: false, ...over,
});
const row = (state, over = {}) => ({ number: 1, head: "aaaaaaa", oid: "aaaaaaaa", state, ...over });
const kind = (r, f = facts(), o = opts) => decide(r, f, o).kind;

test("the gate's dry run maps to readiness", () => {
  assert.equal(readiness(0, "#1 may merge at abc"), "merge");
  assert.equal(readiness(1, "- its head does not contain main's tip; merge main in and let CI run"), "behind");
  assert.equal(readiness(1, "- its head does not contain main's tip\n- abc, after the review, is not a merge; review the head"), "wait");
  assert.equal(readiness(1, "- GitHub reports it unknown"), "wait");
  assert.equal(readiness(2, "gh pr view failed"), "wait");
});

test("a full review blocks on high and medium; a verify only on a listed finding or a new high", () => {
  const f = (severity, origin = "new") => ({ severity, origin });
  assert.equal(blocks({ output: { findings: [f("low")] } }), false);
  assert.equal(blocks({ output: { findings: [f("medium")] } }), true);
  assert.equal(blocks({ since: "x", output: { findings: [f("medium"), f("low")] } }), false);
  assert.equal(blocks({ since: "x", output: { findings: [f("low", "listed")] } }), true);
  assert.equal(blocks({ since: "x", output: { findings: [f("high")] } }), true);
  assert.equal(blocks({ blocks: false, output: { findings: [f("high")] } }), false);
  assert.equal(blocks({ output: { verdict: "clean", findings: [f("medium")] } }), false);
});

// A linear history r1 -> f1 (the fix) -> m1 (a clean merge of main) -> x1 (someone else's commit).
const order = ["r1", "f1", "m1", "x1"];
const g = {
  isAncestor: (a, b) => order.indexOf(a) <= order.indexOf(b),
  onlyCleanMain: (from, to) => order.slice(order.indexOf(from) + 1, order.indexOf(to) + 1).every((c) => c === "m1"),
};
const fixed = { kind: "fix", head: "r1", pushedHead: "f1", startedAt: "2" };

test("which heads get a review, a verify, or nothing", () => {
  assert.deepEqual(route("r1", [], [], g), { need: "review" });
  assert.equal(route("r1", [clean], [], g).need, "none");
  assert.equal(route("f1", [dirty], [fixed], g).need, "verify");
  assert.equal(route("m1", [dirty], [fixed], g).need, "verify", "a clean merge of main after the fix still gets a verify");
  assert.equal(route("x1", [dirty], [fixed], g).need, "review", "a commit nobody accounts for gets a full review");
  assert.equal(route("f1", [dirty], [], g).need, "review", "a push that is not the loop's own fix gets a full review");
  const verified = { ...clean, head: "f1", since: "r1", startedAt: "3" };
  assert.equal(route("m1", [dirty, verified], [fixed], g).review, verified, "the latest review covering the head applies");
});

test("drafts and running CI wait; a dependency update is the owner's", () => {
  assert.equal(kind(row("draft")), "wait");
  assert.equal(kind(row("ci-running")), "wait");
  assert.equal(kind(row("green"), facts({ dependabot: true })), "owner");
  assert.equal(kind(row("green"), facts({ fixPending: true })), "wait");
});

test("green: review, then fix what blocks, then verify, then merge", () => {
  assert.equal(kind(row("green")), "review");
  assert.deepEqual(decide(row("green"), facts({ route: { need: "none", review: dirty } }), opts), { kind: "fix", task: "review", review: dirty });
  assert.equal(kind(row("green"), facts({ route: { need: "verify", review: dirty, fix: fixed } })), "verify");
  assert.equal(kind(row("green"), facts({ route: { need: "none", review: clean }, ready: "merge" })), "merge");
});

test("the loop stops at the round limit and on a fix that changed nothing", () => {
  const blocked = { route: { need: "none", review: dirty } };
  assert.equal(kind(row("green"), facts({ ...blocked, rounds: { review: 3, ci: 0, conflict: 0 } })), "owner");
  assert.equal(kind(row("green"), facts({ ...blocked, rounds: { review: 2, ci: 3, conflict: 0 } })), "fix", "CI fixes count apart");
  assert.equal(kind(row("green"), facts({ ...blocked, fixOn: () => ({ output: { outcome: "blocked", summary: "Disagree." } }) })), "owner");
  assert.equal(kind(row("green"), facts({ failed: { review: 2, fix: 0 } })), "owner");
});

test("ready: a hold label waits, owner paths and shadow runs do not merge", () => {
  const ready = { route: { need: "none", review: clean }, ready: "merge" };
  assert.equal(kind(row("green"), facts({ ...ready, hold: true })), "wait");
  assert.equal(kind(row("green"), facts({ ...ready, ownerMerges: true })), "owner");
  assert.equal(kind(row("green"), facts(ready), { ...opts, merge: false }), "ready");
  assert.equal(kind(row("behind-main"), facts({ route: { need: "none", review: clean }, ready: "behind" })), "merge-main");
});

test("CI, the shepherd and agent instructions are the owner's to merge", () => {
  for (const p of [".github/workflows/ci.yml", ".claude/skills/review-pr/SKILL.md", "CLAUDE.md", "scripts/hooks/guard-briefs.mjs", "scripts/merge-pr.mjs", "scripts/shepherd.mjs"])
    assert.ok(OWNER_MERGES.test(p), p);
  for (const p of ["crates/ersa-core/src/lib.rs", "docs/briefs/FLOW-X.md", "scripts/check.mjs", "apps/shell/src/App.tsx"]) assert.ok(!OWNER_MERGES.test(p), p);
});

test("merging main in retries failures, and a conflict goes to one fix", () => {
  const behind = { route: { need: "none", review: clean }, ready: "behind" };
  assert.equal(kind(row("behind-main"), facts({ ...behind, mainHere: { failures: 2 } })), "merge-main");
  assert.equal(kind(row("behind-main"), facts({ ...behind, mainHere: { failures: 3, error: "500" } })), "owner");
  assert.deepEqual(decide(row("conflicting"), facts({ mainHere: { conflict: true } }), opts), { kind: "fix", task: "conflict" });
  assert.equal(kind(row("conflicting"), facts({ mainHere: { conflict: true }, fixHere: true })), "owner");
});

test("red CI reruns once per head, then gets one fix", () => {
  assert.equal(kind(row("ci-red", { signature: "x" })), "rerun");
  assert.equal(kind(row("ci-red", { signature: "x", shared: true }), facts({ rerunHere: true })), "owner");
  assert.deepEqual(decide(row("ci-red", { signature: "x" }), facts({ rerunHere: true }), opts), { kind: "fix", task: "ci" });
  assert.equal(kind(row("ci-red", { signature: "x" }), facts({ rerunHere: true, fixHere: true })), "owner");
});

test("a fix's kind comes from its task", () => {
  assert.equal(fixKind({ task: ciTask("1", "x") }), "ci");
  assert.equal(fixKind({ task: "GitHub reports this branch conflicts with main." }), "conflict");
  assert.equal(fixKind({ task: "The merge-time review of ..." }), "review");
  assert.equal(fixKind({}), "review");
});

test("the notes comment carries the marker, the findings and the owner's reason", () => {
  const c = notesComment(dirty, "findings remain after 3 fix rounds");
  assert.match(c, /^<!-- shepherd -->\n\*\*PR shepherd\*\* · review of r1/);
  assert.match(c, /block the merge[^]*- \*\*medium\*\*, `a.mjs:1`: Breaks\./);
  assert.match(c, /\*\*Needs the owner:\*\* findings remain after 3 fix rounds/);
  assert.match(notesComment(clean), /No findings\./);
  assert.match(notesComment(undefined, "a dependency update"), /no review yet[^]*Needs the owner/);
});
