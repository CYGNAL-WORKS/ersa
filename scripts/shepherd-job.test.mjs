// Tests for shepherd-job.mjs's briefs and command lines, run without Claude Code.
// Run: node --test scripts/shepherd-job.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { JOBS, claudeArgs, findingsTask, fixPrompt, issueOf, needsPlanning, reviewPrompt } from "./shepherd-job.mjs";

const pr = { number: "118", title: "Sessions tests", branch: "fix/title-race", head: "386eca0123456789", issue: "60" };

test("the issue comes from the Fixes line", () => {
  assert.equal(issueOf("Does a thing.\n\nFixes #60"), "60");
  assert.equal(issueOf("closes #7"), "7");
  assert.equal(issueOf("See #60"), undefined);
  assert.equal(issueOf(null), undefined);
});

test("a review brief names the pull request and forbids acting on findings", () => {
  const p = reviewPrompt(pr);
  assert.match(p, /#118 \("Sessions tests"\), branch `fix\/title-race`, which fixes issue #60/);
  assert.match(p, /detached at the pull request's head, 386eca0/);
  assert.match(p, /review-pr skill as the orchestrator at merge time, steps 1 to 4/);
  assert.match(p, /Do not fix, commit, push/);
});

test("a brief for a pull request with no issue says to judge it by its description", () => {
  assert.match(reviewPrompt({ ...pr, issue: undefined }), /names no issue; judge it against its description \(`gh pr view 118`\)/);
});

test("a fix brief carries the task and leaves the push to the runner", () => {
  const p = fixPrompt(pr, "  - fix the race\n");
  assert.match(p, /Do not push: this job's runner pushes your commits/);
  assert.match(p, /## Task\n\n- fix the race$/);
});

test("a findings task lists medium and high findings, and low ones apart", () => {
  const task = findingsTask(
    {
      findings: [
        { severity: "medium", location: "a.rs:3", defect: "Leaks a thread.", scenario: "each timeout leaves one" },
        { severity: "low", location: "b.rs:9", defect: "Odd name." },
      ],
    },
    "https://github.com/o/r/pull/118"
  );
  assert.match(task, /review of https:\/\/github.com\/o\/r\/pull\/118/);
  assert.match(task, /- \*\*medium\*\*, a.rs:3: Leaks a thread. Failure: each timeout leaves one/);
  assert.match(task, /only if trivial[^]*- b.rs:9: Odd name./);
  assert.doesNotMatch(findingsTask({ findings: [] }, "x"), /trivial/);
});

test("a fix to a planning file runs as planning; the inbox alone does not count", () => {
  assert.equal(needsPlanning(["scripts/pr-queue.mjs", ".claude/skills/shepherd-prs/SKILL.md"]), true);
  assert.equal(needsPlanning(["docs/briefs/FLOW-NEW-SESSION.md"]), true);
  assert.equal(needsPlanning(["CLAUDE.md"]), true);
  assert.equal(needsPlanning(["docs/briefs/INBOX.md", "crates/x/src/lib.rs"]), false);
});

test("neither job may push, merge or write on GitHub; only a fix may commit", () => {
  const review = claudeArgs(JOBS.review, "p");
  const fix = claudeArgs(JOBS.fix, "p");
  const denied = (args) => args.slice(args.indexOf("--disallowedTools") + 1);

  assert.ok(denied(review).includes("Bash(git commit:*)"));
  assert.ok(!denied(fix).includes("Bash(git commit:*)"));
  for (const args of [review, fix]) {
    for (const rule of ["Bash(git push:*)", "Bash(gh pr merge:*)", "Bash(gh pr comment:*)", "Bash(gh api:*)", "PowerShell"]) assert.ok(denied(args).includes(rule), rule);
    assert.equal(args[args.indexOf("--output-format") + 1], "json");
    assert.ok(JSON.parse(args[args.indexOf("--json-schema") + 1]).required.length);
  }
});
