// Tests for hooks/guard-polling.mjs: which loops count as polling GitHub, and
// which tool calls the hook reads them from.
// Run: node --test scripts/guard-polling.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { polls, scriptsIn } from "./hooks/guard-polling.mjs";

const HOOK = join(fileURLToPath(import.meta.url), "..", "hooks", "guard-polling.mjs");
const ROOT = join(fileURLToPath(import.meta.url), "..", "..");

function exitCode(payload) {
  return spawnSync(process.execPath, [HOOK], { input: JSON.stringify({ cwd: ROOT, ...payload }) }).status;
}

const WATCHER = `while true; do
  for n in "$@"; do gh pr view "$n" --json state,statusCheckRollup; done
  sleep 300
done`;

test("finds loops that repeat without a fixed count, call gh and sleep", () => {
  for (const t of [
    WATCHER,
    "until gh pr checks 5 | grep -q pass; do sleep 30; done",
    'while ($true) { & "C:\\Program Files\\GitHub CLI\\gh.exe" pr view 5; Start-Sleep 60 }',
    "do { gh api repos/o/r/pulls; Start-Sleep -Seconds 30 } while ($open)",
    'for (;;) { execSync("gh run list"); await new Promise((r) => setTimeout(r, 1000)); }',
  ]) {
    assert.ok(polls(t), t);
  }
});

test("passes a loop with a fixed count, a wait gh ends itself, and a loop without gh", () => {
  for (const t of [
    "for i in 1 2 3; do gh api rate_limit; sleep 20; done",
    "gh pr checks 5 --watch",
    "until grep -q Ready dev.log; do sleep 0.5; done",
    "while true; do gh pr view 5; done", // no sleep: a mistake, but not a poll
    "high api while sleep", // gh only as part of another word
  ]) {
    assert.ok(!polls(t), t);
  }
});

test("blocks a polling command from Bash, PowerShell or Monitor", () => {
  assert.equal(exitCode({ tool_input: { command: WATCHER } }), 2);
  assert.equal(exitCode({ tool_input: { command: "gh pr checks 5 --watch" } }), 0);
});

test("blocks a command that runs a polling script, in either path form", () => {
  const dir = mkdtempSync(join(tmpdir(), "guard-polling-"));
  try {
    const script = join(dir, "watch-prs.sh");
    writeFileSync(script, WATCHER);
    const posix = script.replace(/\\/g, "/").replace(/^([A-Za-z]):/, (_, d) => `/${d.toLowerCase()}`);
    assert.equal(exitCode({ tool_input: { command: `bash "${script}" 252 253` } }), 2);
    assert.equal(exitCode({ tool_input: { command: `bash ${posix} 252` } }), 2);
    writeFileSync(script, "gh pr view 5");
    assert.equal(exitCode({ tool_input: { command: `bash ${script}` } }), 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("names each shell script a command runs", () => {
  assert.deepEqual(scriptsIn("bash ./a.sh 1 && pwsh -File b.ps1", "/r").map((p) => p.replace(/\\/g, "/").replace(/^[A-Za-z]:/, "")), ["/r/a.sh", "/r/b.ps1"]);
});

test("blocks writing a polling script outside the repository or under .scratch/", () => {
  assert.equal(exitCode({ tool_input: { file_path: join(tmpdir(), "watch.sh"), content: WATCHER } }), 2);
  assert.equal(exitCode({ tool_input: { file_path: join(ROOT, ".scratch", "watch.sh"), content: WATCHER } }), 2);
  assert.equal(exitCode({ tool_input: { file_path: join(tmpdir(), "watch.sh"), old_string: "x", new_string: WATCHER } }), 2);
});

test("passes writing the repository's own scripts, which review covers", () => {
  assert.equal(exitCode({ tool_input: { file_path: join(ROOT, "scripts", "poller.mjs"), content: WATCHER } }), 0);
});

test("passes input it cannot read", () => {
  assert.equal(spawnSync(process.execPath, [HOOK], { input: "not json" }).status, 0);
});
