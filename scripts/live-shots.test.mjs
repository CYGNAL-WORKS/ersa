// Tests for live-shots.mjs, against a throwaway repository and remote.
// Run: node --test scripts/live-shots.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";

import { RESULTS, githubRepo, publishShots, screenshots } from "./live-shots.mjs";

const roots = [];
after(() => roots.forEach((r) => rmSync(r, { recursive: true, force: true })));

function git(cwd, ...args) {
  const r = spawnSync("git", args, { cwd, encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr);
  return r.stdout.trim();
}

// A checkout on `feature` whose origin is a bare repository, with GitHub's URL rewritten to it.
function checkout() {
  const base = mkdtempSync(join(tmpdir(), "live-shots-test-"));
  roots.push(base);
  const bare = join(base, "remote.git");
  const work = join(base, "work");
  git(base, "init", "--bare", "-q", bare);
  git(base, "init", "-q", "-b", "feature", work);
  for (const [key, value] of [
    ["user.name", "Test"],
    ["user.email", "test@ersa.invalid"],
    [`url.${bare.replaceAll("\\", "/")}.insteadOf`, "https://github.com/owner/name.git"],
  ]) {
    git(work, "config", key, value);
  }
  git(work, "remote", "add", "origin", "https://github.com/owner/name.git");
  writeFileSync(join(work, "README"), "x");
  git(work, "add", "README");
  git(work, "commit", "-q", "-m", "first");
  return { bare, work };
}

test("githubRepo reads https and SSH remotes, and nothing else", () => {
  assert.equal(githubRepo("https://github.com/CYGNAL-WORKS/ersa.git"), "CYGNAL-WORKS/ersa");
  assert.equal(githubRepo("git@github.com:CYGNAL-WORKS/ersa.git\n"), "CYGNAL-WORKS/ersa");
  assert.equal(githubRepo("https://github.com/CYGNAL-WORKS/ersa"), "CYGNAL-WORKS/ersa");
  assert.equal(githubRepo("C:/elsewhere/remote.git"), null);
});

test("screenshots lists PNGs only, nested, with forward slashes", () => {
  const dir = mkdtempSync(join(tmpdir(), "live-shots-test-"));
  roots.push(dir);
  mkdirSync(join(dir, "b-test"));
  mkdirSync(join(dir, "a-test"));
  writeFileSync(join(dir, "b-test", "01-x.png"), "");
  writeFileSync(join(dir, "a-test", "01-y.png"), "");
  writeFileSync(join(dir, "a-test", "core.log"), "");
  assert.deepEqual(screenshots(dir), ["a-test/01-y.png", "b-test/01-x.png"]);
  assert.deepEqual(screenshots(join(dir, "missing")), []);
});

test("publishShots pushes the screenshots alone to live-shots/<branch> and links them by commit", () => {
  const { bare, work } = checkout();
  mkdirSync(join(work, RESULTS, "smoke"), { recursive: true });
  writeFileSync(join(work, RESULTS, "smoke", "01-new-session.png"), "png");
  writeFileSync(join(work, RESULTS, "smoke", "core.log"), "log");

  const markdown = publishShots({ root: work });

  const pushed = git(bare, "rev-parse", "refs/heads/live-shots/feature");
  assert.equal(markdown, `![smoke/01-new-session.png](https://raw.githubusercontent.com/owner/name/${pushed}/smoke/01-new-session.png)`);
  assert.equal(git(bare, "ls-tree", "-r", "--name-only", pushed), "smoke/01-new-session.png");
  assert.equal(git(bare, "rev-list", "--count", pushed), "1", "none of the pull request's own history");
  // The checkout's index and branch are as they were.
  assert.equal(git(work, "status", "--porcelain", "--untracked-files=no"), "");
  assert.equal(git(work, "rev-list", "--count", "HEAD"), "1");

  // Publishing again builds on the first commit, so links to it keep resolving.
  writeFileSync(join(work, RESULTS, "smoke", "01-new-session.png"), "png again");
  publishShots({ root: work });
  const second = git(bare, "rev-parse", "refs/heads/live-shots/feature");
  assert.notEqual(second, pushed);
  assert.equal(git(bare, "rev-parse", `${second}^`), pushed);
});

test("publishShots refuses when there is nothing to publish", () => {
  const { work } = checkout();
  assert.throws(() => publishShots({ root: work }), /No screenshots/);
});
