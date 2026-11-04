// Tests for shepherd-job.mjs's briefs and command lines, run without Claude Code.
// Run: node --test scripts/shepherd-job.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { JOBS, blocking, claudeArgs, needsPlanning } from "./shepherd-job.mjs";

const data = "D:/x/118-386eca0-review.data";

test("blocking: a review on high and medium, a verify on listed findings and new highs", () => {
  const f = [
    { severity: "high", origin: "new" },
    { severity: "medium", origin: "new" },
    { severity: "low", origin: "listed" },
    { severity: "low", origin: "new" },
  ];
  assert.deepEqual(blocking(f, false), [f[0], f[1]]);
  assert.deepEqual(blocking(f, true), [f[0], f[2]]);
  assert.deepEqual(blocking(undefined, false), []);
});

test("a fix to a planning file runs as planning; the inbox alone does not count", () => {
  assert.equal(needsPlanning(["scripts/pr-queue.mjs", ".claude/skills/shepherd-prs/SKILL.md"]), true);
  assert.equal(needsPlanning(["docs/briefs/FLOW-NEW-SESSION.md"]), true);
  assert.equal(needsPlanning(["CLAUDE.md"]), true);
  assert.equal(needsPlanning(["docs/briefs/INBOX.md", "crates/x/src/lib.rs"]), false);
});

test("jobs run with hooks off, can read their data folder, and may not push, merge or write on GitHub", () => {
  const review = claudeArgs(JOBS.review, "p", data);
  const fix = claudeArgs(JOBS.fix, "p", data);
  const denied = (args) => args.slice(args.indexOf("--disallowedTools") + 1);
  assert.ok(denied(review).includes("Bash(git commit:*)"));
  assert.ok(!denied(fix).includes("Bash(git commit:*)"));
  for (const args of [review, fix]) {
    for (const rule of ["Bash(git push:*)", "Bash(gh pr merge:*)", "Bash(gh pr comment:*)", "Bash(gh api:*)", "PowerShell"]) assert.ok(denied(args).includes(rule), rule);
    assert.deepEqual(JSON.parse(args[args.indexOf("--settings") + 1]), { disableAllHooks: true });
    assert.ok(args.includes("--strict-mcp-config"));
    assert.equal(args[args.indexOf("--add-dir") + 1], data);
    assert.equal(args[args.indexOf("--output-format") + 1], "json");
    assert.ok(JSON.parse(args[args.indexOf("--json-schema") + 1]).required.length);
  }
});
