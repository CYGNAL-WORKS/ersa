// Tests for daily-build.mjs against real git repositories in a temporary
// folder: an "origin", the owner's clone of it, and the script's own checkout.
// Nothing is built. Run: node --test scripts/daily-build.test.mjs

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";

import { prepareCheckout, verifyAtMain } from "./daily-build.mjs";

const roots = [];
after(() => roots.forEach((r) => rmSync(r, { recursive: true, force: true })));

function git(cwd, ...args) {
  return execFileSync("git", args, { cwd, encoding: "utf8" }).trim();
}

function commit(cwd, file, text) {
  writeFileSync(join(cwd, file), text);
  git(cwd, "add", file);
  git(cwd, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q", "-m", file);
}

// An origin with one commit on main, and the owner's clone of it.
function repos() {
  const root = mkdtempSync(join(tmpdir(), "daily-build-test-"));
  roots.push(root);
  const origin = join(root, "origin");
  git(root, "init", "-q", "-b", "main", origin);
  commit(origin, "a.txt", "one");
  const owner = join(root, "owner");
  git(root, "clone", "-q", origin, owner);
  return { root, origin, owner, checkout: join(root, "daily") };
}

test("a checkout at origin/main with no changes may be built", () => {
  const { owner, checkout } = repos();
  prepareCheckout(owner, checkout);
  assert.doesNotThrow(() => verifyAtMain(checkout));
});

test("a checkout with a commit origin/main lacks is refused", () => {
  const { owner, checkout } = repos();
  prepareCheckout(owner, checkout);
  commit(checkout, "b.txt", "local");
  assert.throws(() => verifyAtMain(checkout), /isn't at origin\/main/);
});

test("a checkout behind origin/main is refused", () => {
  const { origin, owner, checkout } = repos();
  prepareCheckout(owner, checkout);
  commit(origin, "b.txt", "two");
  git(checkout, "fetch", "-q", "origin", "main");
  assert.throws(() => verifyAtMain(checkout), /isn't at origin\/main/);
});

test("a checkout with a changed or untracked file is refused", () => {
  const { owner, checkout } = repos();
  prepareCheckout(owner, checkout);
  writeFileSync(join(checkout, "a.txt"), "edited");
  assert.throws(() => verifyAtMain(checkout), /has changes/);
  git(checkout, "checkout", "--", "a.txt");
  writeFileSync(join(checkout, "new.txt"), "untracked");
  assert.throws(() => verifyAtMain(checkout), /has changes/);
});

test("preparing again moves a stale, changed checkout to the new origin/main", () => {
  const { origin, owner, checkout } = repos();
  prepareCheckout(owner, checkout);
  commit(origin, "b.txt", "two");
  writeFileSync(join(checkout, "a.txt"), "edited");
  writeFileSync(join(checkout, "stray.txt"), "untracked");
  prepareCheckout(owner, checkout);
  assert.equal(git(checkout, "rev-parse", "HEAD"), git(origin, "rev-parse", "main"));
  assert.ok(!existsSync(join(checkout, "stray.txt")));
  assert.doesNotThrow(() => verifyAtMain(checkout));
});

test("the owner's working tree is untouched", () => {
  const { origin, owner, checkout } = repos();
  writeFileSync(join(owner, "a.txt"), "owner's edit");
  const before = git(owner, "rev-parse", "HEAD");
  commit(origin, "b.txt", "two");
  prepareCheckout(owner, checkout);
  assert.equal(git(owner, "rev-parse", "HEAD"), before);
  assert.equal(git(owner, "status", "--porcelain"), "M a.txt");
});

test("run from another clone, a checkout made by the first one still moves to the new origin/main", () => {
  const { root, origin, owner, checkout } = repos();
  const second = join(root, "second");
  git(root, "clone", "-q", origin, second);
  prepareCheckout(owner, checkout);
  commit(origin, "b.txt", "two");
  prepareCheckout(second, checkout);
  assert.equal(git(checkout, "rev-parse", "HEAD"), git(origin, "rev-parse", "main"));
  assert.doesNotThrow(() => verifyAtMain(checkout));
});
