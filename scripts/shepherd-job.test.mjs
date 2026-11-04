// Tests for shepherd-job.mjs's briefs and command lines, run without Claude Code.
// Run: node --test scripts/shepherd-job.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { descendants } from "./kill-tree.mjs";
import { JOBS, blocking, claudeArgs, instructionPaths, jobRunning, needsPlanning, planningPaths } from "./shepherd-job.mjs";

const data = "D:/x/118-386eca0-review.data";

test("blocking: high and medium, listed or new", () => {
  const f = [
    { severity: "high", origin: "new" },
    { severity: "medium", origin: "new" },
    { severity: "low", origin: "listed" },
    { severity: "medium", origin: "listed" },
    { severity: "low", origin: "new" },
  ];
  assert.deepEqual(blocking(f), [f[0], f[1], f[3]]);
  assert.deepEqual(blocking(undefined), []);
});

test("a fix to a planning file runs as planning; the inbox alone does not count", () => {
  assert.equal(needsPlanning(["scripts/pr-queue.mjs", ".claude/skills/shepherd-prs/SKILL.md"]), true);
  assert.equal(needsPlanning(["docs/briefs/FLOW-NEW-SESSION.md"]), true);
  assert.equal(needsPlanning(["CLAUDE.md"]), true);
  assert.equal(needsPlanning(["docs/briefs/INBOX.md", "crates/x/src/lib.rs"]), false);
  assert.deepEqual(planningPaths(["docs/adr/0001-x.md", "crates/core/CLAUDE.md", "docs/briefs/protocol/fixtures/a.json", "scripts/shepherd.mjs"]), ["docs/adr/0001-x.md", "crates/core/CLAUDE.md"]);
});

test("a review takes every instruction file the pull request changed from main, in any folder", () => {
  const files = ["CLAUDE.md", "scripts/CLAUDE.md", "apps/CLAUDE.local.md", "apps/shell/.claude/settings.json", ".claude/skills/x/SKILL.md", "scripts/shepherd.mjs", "docs/NOTCLAUDE.md.txt"];
  assert.deepEqual(instructionPaths(files), files.slice(0, 5));
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

test("a job's lock holds while its process lives, and not past the longest a job can run", () => {
  const now = 10_000_000;
  assert.equal(jobRunning(null, now, () => true), false);
  assert.equal(jobRunning({ pid: 1, at: now - 1000 }, now, () => true), true);
  assert.equal(jobRunning({ pid: 1, at: now - 1000 }, now, () => false), false, "a killed job's lock");
  assert.equal(jobRunning({ pid: 1, at: now - 3 * 60 * 60 * 1000 }, now, () => true), false, "a reused process id");
});

test("a process tree is walked by parent id, past exited parents, never into older processes", () => {
  const since = new Date(1000_000);
  const procs = [
    { pid: 20, ppid: 10, created: 1000_500 }, // claude's shell; claude (10) has exited
    { pid: 30, ppid: 20, created: 1001_000 }, // cargo under it
    { pid: 40, ppid: 10, created: 500_000 }, // an older process whose parent id was reused
    { pid: 50, ppid: 1, created: 1002_000 },
  ];
  assert.deepEqual(descendants(10, procs, since).sort(), [10, 20, 30]);
});
