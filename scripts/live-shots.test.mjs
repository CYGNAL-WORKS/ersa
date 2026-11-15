// Tests for live-shots.mjs, against a throwaway repository and remote.
// Run: node --test scripts/live-shots.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";

import { BACKEND_RECORD, RESULTS, githubRepo, publishShots, screenshots } from "./live-shots.mjs";

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
  mkdirSync(join(dir, "a-test", "attachments"));
  writeFileSync(join(dir, "a-test", "attachments", "y-0123abcd.png"), "");
  assert.deepEqual(screenshots(dir), ["a-test/01-y.png", "b-test/01-x.png"]);
  assert.deepEqual(screenshots(join(dir, "missing")), []);
});

test("publishShots pushes the screenshots alone to live-shots/<branch> and links them by commit", () => {
  const { bare, work } = checkout();
  mkdirSync(join(work, RESULTS, "smoke"), { recursive: true });
  writeFileSync(join(work, RESULTS, "smoke", "01-new-session.png"), "png");
  writeFileSync(join(work, RESULTS, "smoke", "core.log"), "log");
  writeFileSync(join(work, RESULTS, "smoke", BACKEND_RECORD), "stand-in\n");

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

// Two tests' results, one that ran the stand-in and one whose record says `record` (or none, for `null`).
function mixedResults(record) {
  const { bare, work } = checkout();
  mkdirSync(join(work, RESULTS, "smoke"), { recursive: true });
  writeFileSync(join(work, RESULTS, "smoke", "01-new-session.png"), "png");
  writeFileSync(join(work, RESULTS, "smoke", BACKEND_RECORD), "stand-in");
  mkdirSync(join(work, RESULTS, "tools"), { recursive: true });
  writeFileSync(join(work, RESULTS, "tools", "01-tool-call.png"), "png");
  if (record !== null) writeFileSync(join(work, RESULTS, "tools", BACKEND_RECORD), record);
  return { bare, work };
}

// Nothing reached the remote: no branch, and no objects at all.
function assertNothingPushed(bare, work) {
  assert.equal(git(bare, "for-each-ref", "refs/heads/live-shots"), "");
  assert.equal(git(bare, "count-objects", "-v").match(/^count: (\d+)/m)?.[1], "0");
  assert.equal(git(bare, "count-objects", "-v").match(/^in-pack: (\d+)/m)?.[1], "0");
  // Nor was the screenshot written to the checkout's own object store.
  const blob = spawnSync("git", ["cat-file", "-e", git(work, "hash-object", join(work, RESULTS, "tools", "01-tool-call.png"))], { cwd: work });
  assert.notEqual(blob.status, 0, "the refused screenshot is not in the object store");
}

test("publishShots refuses the whole run when a screenshot came from the real backend", () => {
  const { bare, work } = mixedResults("claude\n");
  assert.throws(
    () => publishShots({ root: work }),
    (e) => {
      assert.match(e.message, /tools\/01-tool-call\.png: ran the real backend \(claude\)/);
      assert.doesNotMatch(e.message, /smoke\/01-new-session/);
      // Says what to do.
      assert.match(e.message, /Delete .*tools/);
      assert.match(e.message, /in words in the pull request's Evidence/);
      return true;
    },
  );
  assertNothingPushed(bare, work);
});

test("publishShots refuses a screenshot whose backend is not recorded", () => {
  const { bare, work } = mixedResults(null);
  assert.throws(() => publishShots({ root: work }), /tools\/01-tool-call\.png: no record of the backend it ran/);
  assertNothingPushed(bare, work);
});

test("publishShots refuses any backend but the stand-in", () => {
  const { bare, work } = mixedResults("copilot");
  assert.throws(() => publishShots({ root: work }), /tools\/01-tool-call\.png: ran the real backend \(copilot\)/);
  assertNothingPushed(bare, work);
});

// The harness writes the record this script reads; the two must agree on its name and on the stand-in's value.
test("the live-check harness writes the backend record under the name the script reads", () => {
  const harness = readFileSync(join(import.meta.dirname, "..", "apps", "shell", "live", "harness.ts"), "utf8");
  assert.ok(harness.includes(`writeFileSync(testInfo.outputPath("${BACKEND_RECORD}"), backend)`), "the harness writes the record");
  assert.ok(harness.includes(`backend: ["stand-in", { option: true }]`), "the stand-in is named as the script names it");
});
