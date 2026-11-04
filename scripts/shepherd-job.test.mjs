// Tests for shepherd-job.mjs's briefs, command lines, push and records, run
// without Claude Code; the push runs against a real temporary repository.
// Run: node --test scripts/shepherd-job.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  JOBS, NEVER, TOOLS, claudeArgs, findingsTask, fixPrompt, gitFingerprint, issueOf, jobEnv, needsPlanning, pushFix, recordName, reviewPrompt, runTree,
} from "./shepherd-job.mjs";

const pr = { number: "118", title: "Sessions tests", branch: "fix/title-race", head: "386eca0123456789", issue: "60" };

test("the issue comes from the Fixes line", () => {
  assert.equal(issueOf("Does a thing.\n\nFixes #60"), "60");
  assert.equal(issueOf("closes #7"), "7");
  assert.equal(issueOf("Resolves #9"), "9");
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

test("a fix to a planning file runs as planning; the inbox and fixtures alone do not count", () => {
  assert.equal(needsPlanning(["scripts/pr-queue.mjs", ".claude/skills/shepherd-prs/SKILL.md"]), true);
  assert.equal(needsPlanning(["docs/briefs/FLOW-NEW-SESSION.md"]), true);
  assert.equal(needsPlanning(["CLAUDE.md"]), true);
  assert.equal(needsPlanning(["crates/core/CLAUDE.md"]), true);
  assert.equal(needsPlanning(["docs/GLOSSARY.md"]), true);
  assert.equal(needsPlanning(["docs/adr/0001-x.md"]), true);
  assert.equal(needsPlanning(["scripts/hooks/guard-briefs.mjs"]), true);
  assert.equal(needsPlanning([".claude/settings.json"]), true);
  assert.equal(needsPlanning(["docs/briefs/INBOX.md", "crates/x/src/lib.rs"]), false);
  assert.equal(needsPlanning(["docs/briefs/protocol/fixtures/a.jsonl", ".claude/role.local"]), false);
});

test("a fix to planning files runs as planning, any other job as implementing, all on one build directory", () => {
  assert.equal(jobEnv("fix", ["CLAUDE.md"], { A: "1" }, "w").ERSA_ROLE, "planning");
  assert.equal(jobEnv("fix", ["src/a.rs"], {}, "w").ERSA_ROLE, "implementing");
  assert.equal(jobEnv("review", ["CLAUDE.md"], {}, "w").ERSA_ROLE, "implementing");
  assert.equal(jobEnv("review", [], { A: "1" }, "w").A, "1");
  assert.equal(jobEnv("review", [], {}, "w").CARGO_TARGET_DIR, join("w", "target"));
});

test("only a job with a result and no error uses up its head", () => {
  assert.equal(recordName("5-abc-fix", { output: { outcome: "committed" }, error: null }, 9), "5-abc-fix.json");
  assert.equal(recordName("5-abc-fix", { output: { outcome: "committed" }, error: "push failed: x" }, 9), "5-abc-fix.9.failed.json");
  assert.equal(recordName("5-abc-fix", { output: null, error: "x" }, 9), "5-abc-fix.9.failed.json");
});

test("both jobs run opus at medium effort, with edits accepted and no project hooks or MCP servers", () => {
  for (const job of [JOBS.review, JOBS.fix]) {
    assert.equal(job.model, "opus");
    assert.equal(job.effort, "medium");
    const args = claudeArgs(job, "p");
    assert.equal(args[args.indexOf("--permission-mode") + 1], "acceptEdits");
    assert.deepEqual(JSON.parse(args[args.indexOf("--settings") + 1]), { disableAllHooks: true });
    assert.ok(args.includes("--strict-mcp-config"));
    const allowed = args.slice(args.indexOf("--allowedTools") + 1, args.indexOf("--disallowedTools"));
    assert.deepEqual(allowed, ["Read", "Grep", "Glob", "Agent", "Skill", "Edit", "Write", "Bash"]);
    assert.deepEqual(allowed, TOOLS);
  }
});

test("neither job may push, merge or write on GitHub; only a fix may commit", () => {
  const review = claudeArgs(JOBS.review, "p");
  const fix = claudeArgs(JOBS.fix, "p");
  const denied = (args) => args.slice(args.indexOf("--disallowedTools") + 1);

  assert.ok(denied(review).includes("Bash(git commit:*)"));
  assert.ok(!denied(fix).includes("Bash(git commit:*)"));
  const rules = [
    "PowerShell", "WebFetch", "WebSearch", "Bash(curl:*)", "Bash(git push:*)",
    "Bash(gh pr merge:*)", "Bash(gh pr comment:*)", "Bash(gh pr review:*)", "Bash(gh pr close:*)", "Bash(gh pr reopen:*)", "Bash(gh pr edit:*)", "Bash(gh pr create:*)", "Bash(gh pr ready:*)",
    "Bash(gh issue comment:*)", "Bash(gh issue create:*)", "Bash(gh issue close:*)", "Bash(gh issue edit:*)", "Bash(gh api:*)",
    "Bash(gh workflow:*)", "Bash(gh run rerun:*)", "Bash(gh run cancel:*)", "Bash(gh release:*)", "Bash(gh repo:*)", "Bash(gh label:*)",
    "Bash(gh secret:*)", "Bash(gh variable:*)", "Bash(gh gist:*)", "Bash(gh auth:*)",
  ];
  assert.deepEqual([...NEVER].sort(), [...rules].sort());
  for (const args of [review, fix]) {
    for (const rule of rules) assert.ok(denied(args).includes(rule), rule);
    assert.equal(args[args.indexOf("--output-format") + 1], "json");
    assert.ok(JSON.parse(args[args.indexOf("--json-schema") + 1]).required.length);
  }
});

// A real origin, a clone with a branch pushed to it, and a commit helper.
function repo() {
  const root = mkdtempSync(join(tmpdir(), "shepherd-job-"));
  const env = { ...process.env, GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t" };
  const git = (cwd, ...args) => {
    const r = spawnSync("git", args, { cwd, env, encoding: "utf8" });
    assert.equal(r.status, 0, `git ${args.join(" ")}: ${r.stderr}`);
    return r.stdout.trim();
  };
  git(root, "init", "-q", "--bare", "origin.git");
  git(root, "clone", "-q", "origin.git", "work");
  const work = join(root, "work");
  const commit = (cwd, name) => {
    writeFileSync(join(cwd, `${name}.txt`), name);
    git(cwd, "add", "-A");
    git(cwd, "commit", "-q", "-m", name);
    return git(cwd, "rev-parse", "HEAD");
  };
  const head = commit(work, "a");
  git(work, "push", "-q", "origin", "HEAD:feat");
  const origin = () => git(root, "--git-dir", "origin.git", "rev-parse", "feat");
  return { root, work, git, commit, head, pr: { head, branch: "feat" }, origin };
}

function withRepo(body) {
  const r = repo();
  try {
    body(r);
  } finally {
    rmSync(r.root, { recursive: true, force: true });
  }
}

test("a fix that adds to its head is pushed to the branch", () =>
  withRepo((r) => {
    const c = r.commit(r.work, "b");
    assert.deepEqual(pushFix(r.work, r.pr, "implementing"), { commitHead: c, pushedHead: c, uncommitted: 0 });
    assert.equal(r.origin(), c);
  }));

test("a fix with no new commit pushes nothing", () =>
  withRepo((r) => {
    assert.deepEqual(pushFix(r.work, r.pr, "implementing"), { commitHead: r.head, pushedHead: null, uncommitted: 0 });
  }));

test("a fix that rewrote the head's history is not pushed", () =>
  withRepo((r) => {
    r.git(r.work, "checkout", "-q", "--orphan", "other");
    const c = r.commit(r.work, "c");
    const out = pushFix(r.work, r.pr, "implementing");
    assert.equal(out.commitHead, c);
    assert.equal(out.pushedHead, null);
    assert.match(out.error, /rewrote the branch's history/);
    assert.equal(r.origin(), r.head);
  }));

test("a push the branch refuses is an error that names the commits", () =>
  withRepo((r) => {
    r.git(r.root, "clone", "-q", "-b", "feat", "origin.git", "other");
    r.commit(join(r.root, "other"), "moved");
    r.git(join(r.root, "other"), "push", "-q", "origin", "HEAD:feat");
    const c = r.commit(r.work, "b");
    const out = pushFix(r.work, r.pr, "implementing");
    assert.equal(out.commitHead, c);
    assert.equal(out.pushedHead, null);
    assert.match(out.error, /^push failed/);
  }));

test("an implementing fix that changed a planning path is not pushed; a planning one is", () =>
  withRepo((r) => {
    writeFileSync(join(r.work, "CLAUDE.md"), "x");
    const c = r.commit(r.work, "b");
    assert.match(pushFix(r.work, r.pr, "implementing").error, /planning path/);
    assert.equal(r.origin(), r.head);
    assert.equal(pushFix(r.work, r.pr, "planning").pushedHead, c);
  }));

test("a change to the shared git config or hooks changes the fingerprint", () =>
  withRepo((r) => {
    const dir = join(r.work, ".git");
    const before = gitFingerprint(dir);
    assert.equal(gitFingerprint(dir), before);
    writeFileSync(join(dir, "hooks", "pre-push"), "#!/bin/sh\n");
    const hooked = gitFingerprint(dir);
    assert.notEqual(hooked, before);
    r.git(r.work, "config", "remote.origin.pushurl", "elsewhere");
    assert.notEqual(gitFingerprint(dir), hooked);
  }));

test("a job that times out is killed with the processes it started", async () => {
  const root = mkdtempSync(join(tmpdir(), "shepherd-job-"));
  const pidFile = join(root, "pid");
  try {
    // On Windows, Node kills its own children when it exits; cargo and other
    // tools do not, so the grandchild breaks away as theirs would.
    const grandchild = "setInterval(() => {}, 1000)";
    const breakAway = process.platform === "win32";
    const child = [
      `const c = require("child_process").spawn(process.execPath, ["-e", ${JSON.stringify(grandchild)}], { stdio: "ignore", detached: ${breakAway} });`,
      `require("fs").writeFileSync(${JSON.stringify(pidFile)}, String(c.pid));`,
      "setInterval(() => {}, 1000);",
    ].join(" ");
    const r = await runTree(process.execPath, ["-e", child], {}, 1500);
    assert.equal(r.ok, false);
    assert.match(r.err, /timed out/);
    const pid = Number(readFileSync(pidFile, "utf8"));
    await new Promise((ok) => setTimeout(ok, 500));
    assert.throws(() => process.kill(pid, 0), "the grandchild survived");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
