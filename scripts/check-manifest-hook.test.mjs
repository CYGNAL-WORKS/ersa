// Tests for the commit hook, scripts/hooks/check-manifest.mjs. Each case runs the
// hook against a scratch repository whose scripts/manifest.mjs is a stub.
// Run: node --test scripts/check-manifest-hook.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

import { blockingProblems, commitTarget } from "./hooks/check-manifest.mjs";

const HOOK = join(fileURLToPath(import.meta.url), "..", "hooks", "check-manifest.mjs");
const scratch = [];
after(() => scratch.forEach((dir) => rmSync(dir, { recursive: true, force: true })));

test("recognizes a commit and where it runs", () => {
  assert.deepEqual(commitTarget('git commit -m "x"'), { dir: null, all: false });
  assert.deepEqual(commitTarget("git add -A && git commit -qF -"), { dir: null, all: false });
  assert.deepEqual(commitTarget('git -C "D:/a b" commit -m x'), { dir: "D:/a b", all: false });
  assert.deepEqual(commitTarget("git -c user.name=x -C ../w commit -qam msg"), { dir: "../w", all: true });
  assert.deepEqual(commitTarget("git commit --all -m x"), { dir: null, all: true });
  assert.deepEqual(commitTarget('git commit --amend -m "-a in a message"'), { dir: null, all: false });
});

test("a commit after a directory change is not recognized, since where it runs is unknown", () => {
  for (const command of ["cd sub && git commit -m x", "Set-Location sub; git commit -m x", "pushd sub && git -C . commit -m x"]) {
    assert.equal(commitTarget(command), null, command);
  }
  assert.deepEqual(commitTarget("git commit -m x && cd sub"), { dir: null, all: false });
});

test("ignores everything that is not a commit", () => {
  for (const command of ["git status", "git log --oneline", "git commit-tree abc", "echo commit", "git merge main"]) {
    assert.equal(commitTarget(command), null, command);
  }
});

test("open inbox entries alone do not block", () => {
  assert.deepEqual(blockingProblems("## Problems\n\n- 2 open inbox entries need a planning pass\n"), []);
  assert.deepEqual(blockingProblems("## Problems\n\n- 1 open inbox entry need a planning pass\n- x: missing id\n"), [
    "- x: missing id",
  ]);
  assert.deepEqual(blockingProblems("## Problems\n\nNone.\n"), []);
});

// A repository with a committed docs/MANIFEST.md and a stub manifest script
// that writes `manifest` to it (and `adr` to the ADR index, when given), says so
// as the real script does, and exits with `status`. `crash` makes it throw first.
function repo({ manifest = "## Problems\n\nNone.\n", status = 0, adr, crash = false } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "ersa-hook-"));
  scratch.push(dir);
  mkdirSync(join(dir, "scripts"));
  mkdirSync(join(dir, "docs", "adr"), { recursive: true });
  writeFileSync(join(dir, "docs", "MANIFEST.md"), "## Problems\n\nNone.\n");
  writeFileSync(join(dir, "docs", "adr", "README.md"), "index\n");
  writeFileSync(
    join(dir, "scripts", "manifest.mjs"),
    `import { writeFileSync } from "node:fs";\n` +
      `writeFileSync("ran", "");\n` +
      (crash ? `throw new Error("stub crashed");\n` : "") +
      `writeFileSync("docs/MANIFEST.md", ${JSON.stringify(manifest)});\n` +
      (adr ? `writeFileSync("docs/adr/README.md", ${JSON.stringify(adr)});\n` : "") +
      `console.log("wrote docs/MANIFEST.md and the ADR index");\n` +
      `process.exit(${status});\n`
  );
  const git = (...args) => spawnSync("git", ["-C", dir, ...args], { encoding: "utf8" });
  git("init", "-q");
  git("-c", "user.name=t", "-c", "user.email=t@example.com", "add", "docs", "scripts");
  git("-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "init");
  return dir;
}

function hook(command, cwd) {
  const input = JSON.stringify({ tool_name: "Bash", tool_input: { command }, cwd });
  return spawnSync(process.execPath, [HOOK], { input, encoding: "utf8" });
}

test("a clean check allows the commit", () => {
  const dir = repo();
  const run = hook('git commit -m "x"', dir);
  assert.equal(run.status, 0, run.stderr);
  assert.ok(existsSync(join(dir, "ran")));
});

test("a command that is not a commit runs no check", () => {
  const dir = repo();
  assert.equal(hook("git status", dir).status, 0);
  assert.ok(!existsSync(join(dir, "ran")));
});

test("a structural problem blocks the commit and is named", () => {
  const dir = repo({ manifest: "## Problems\n\n- tests cite X-R01, which no brief defines\n", status: 1 });
  const run = hook('git commit -m "x"', dir);
  assert.equal(run.status, 2);
  assert.match(run.stderr, /tests cite X-R01, which no brief defines/);
});

test("open inbox entries alone allow the commit", () => {
  const manifest = "## Problems\n\n- 1 open inbox entry need a planning pass\n";
  const dir = repo({ manifest, status: 1 });
  writeFileSync(join(dir, "docs", "MANIFEST.md"), manifest);
  spawnSync("git", ["-C", dir, "add", "docs"]);
  assert.equal(hook('git commit -m "x"', dir).status, 0);
});

test("a regenerated file left unstaged blocks, unless the commit stages it", () => {
  const dir = repo({ manifest: "## Problems\n\nNone.\n\n<!-- new -->\n" });
  const run = hook('git commit -m "x"', dir);
  assert.equal(run.status, 2);
  assert.match(run.stderr, /docs\/MANIFEST\.md/);
  assert.equal(hook('git commit -am "x"', dir).status, 0);
});

test("a regenerated ADR index left unstaged blocks", () => {
  const dir = repo({ adr: "new index\n" });
  const run = hook('git commit -m "x"', dir);
  assert.equal(run.status, 2);
  assert.match(run.stderr, /docs\/adr\/README\.md/);
});

test("git -C naming another worktree of the session's repository checks that worktree", () => {
  const dir = repo();
  const tree = mkdtempSync(join(tmpdir(), "ersa-hook-wt-"));
  scratch.push(tree);
  rmSync(tree, { recursive: true });
  spawnSync("git", ["-C", dir, "worktree", "add", "-q", "--detach", tree]);
  assert.equal(hook(`git -C "${tree}" commit -m x`, dir).status, 0);
  assert.ok(existsSync(join(tree, "ran")));
  assert.ok(!existsSync(join(dir, "ran")));
});

test("git -C naming another repository runs none of its scripts", () => {
  const own = repo();
  const other = repo({ status: 1 });
  assert.equal(hook(`git -C "${other}" commit -m x`, own).status, 0);
  assert.ok(!existsSync(join(other, "ran")));
  assert.ok(!existsSync(join(own, "ran")));
});

test("a check that crashes blocks, even when the old manifest lists only open inbox entries", () => {
  const manifest = "## Problems\n\n- 1 open inbox entry need a planning pass\n";
  const dir = repo({ crash: true });
  writeFileSync(join(dir, "docs", "MANIFEST.md"), manifest);
  spawnSync("git", ["-C", dir, "add", "docs"]);
  const run = hook('git commit -m "x"', dir);
  assert.equal(run.status, 2);
  assert.match(run.stderr, /stub crashed/);
});

test("a failing check that reports no problem blocks with its stderr", () => {
  const dir = repo({ status: 1 });
  const run = hook('git commit -m "x"', dir);
  assert.equal(run.status, 2);
  assert.match(run.stderr, /manifest check failed/);
});

test("a failing check with only open inbox entries allows the commit", () => {
  const manifest = ["## Problems", "", "- 1 open inbox entry need a planning pass", ""].join("\n");
  const dir = repo({ manifest, status: 1 });
  assert.equal(hook('git commit -am x', dir).status, 0);
});
