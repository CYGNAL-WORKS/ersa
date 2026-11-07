// Tests for hooks/guard-polling.mjs: which loops count as polling GitHub, and
// which tool calls the hook reads them from.
// Run: node --test scripts/guard-polling.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { polls, scriptsIn } from "./hooks/guard-polling.mjs";

const HOOK = join(fileURLToPath(import.meta.url), "..", "hooks", "guard-polling.mjs");
const ROOT = join(fileURLToPath(import.meta.url), "..", "..");

// The hook's exit code, or null when it does not answer within 10 seconds.
function exitCode(payload) {
  return spawnSync(process.execPath, [HOOK], { input: JSON.stringify({ cwd: ROOT, ...payload }), timeout: 10_000 }).status;
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
    "for ((;;)); do gh pr view 5; sleep 30; done",
    "while true; do gh -R o/r pr view 5; sleep 30; done",
    "while true; do gh --repo o/r pr view 5; read -t 30; done",
    'powershell -Command "while ($true) { gh pr view 5; Start-Sleep 60 }"',
    "if ok; then while true; do gh pr view 5; sleep 9; done; fi",
    "powershell -Command while ($true) { gh pr view 5; Start-Sleep 60 }",
    "While ($true) { gh pr view 1; Start-Sleep 30 }",
    "while (1) { gh pr view 5; timeout /t 30 }",
    "while true; do gh project item-list; sleep 1; done",
    "watch -n 30 gh pr view 1",
    "Do { gh pr view 1; Start-Sleep 30 } Until ($done)",
    "For (;;) { gh pr view 1; Start-Sleep 30 }",
    'setInterval(() => execSync("gh pr view 5"), 30000)',
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
    "while true; do thigh pr view; sleep 1; done",
    'git commit -m "Wait on gh pr checks while the review runs; sleep 5 between"',
    'gh pr create --body "poll until done with gh pr view, then sleep"',
    'gh pr comment 5 --body "Fixed: the poller would sleep while CI ran"',
    'git commit -m "Poller: sleep while gh api returns 403"',
    'gh pr create --body "The timer uses setInterval now"',
    'git commit -m "Polling guard: fixes\n\nLoop keywords ignore case;\nwatch -n and setInterval count as repeating; any gh subcommand counts. Runs in a child."',
    'gh pr create --body "Summary\nWhile the review runs, the orchestrator does not sleep."',
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
    assert.equal(exitCode({ tool_input: { command: `"C:\\Program Files\\Git\\usr\\bin\\bash.exe" ${posix} 252` } }), 2);
    assert.equal(exitCode({ tool_input: { command: `cd /x && ${posix} 252` } }), 2);
    for (const cleanup of [`rm ${posix}`, `Remove-Item "${script}"`, `pkill -f ${posix}`, `cat ${posix}`]) {
      assert.equal(exitCode({ tool_input: { command: cleanup } }), 0, cleanup);
    }
    writeFileSync(script, "gh pr view 5");
    assert.equal(exitCode({ tool_input: { command: `bash ${script}` } }), 0);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("names each local shell script a command runs, never a network path", () => {
  assert.deepEqual(scriptsIn("bash ./a.sh 1 && pwsh -File b.ps1", "/r").map((p) => p.replace(/\\/g, "/").replace(/^[A-Za-z]:/, "")), ["/r/a.sh", "/r/b.ps1"]);
  // Only Windows opens a network path from such a name; elsewhere both are local paths.
  if (process.platform === "win32") assert.deepEqual(scriptsIn("bash \\\\host.example\\s\\x.sh; bash //host.example/s/y.sh", "/r"), []);
  for (const c of ["nohup ./w.sh &", "setsid ./w.sh", "timeout 5 ./w.sh", "source w.sh", ". ./w.sh", "pwsh -ExecutionPolicy Bypass -File w.ps1", "Start-Process -FilePath C:\\t\\w.ps1"]) {
    assert.equal(scriptsIn(c, "/r").length, 1, c);
  }
});

test("reads a Git Bash drive path as a drive only on Windows", () => {
  const as = (platform) => scriptsIn("bash /a/w.sh", "/r", platform)[0].replace(/\\/g, "/").toLowerCase();
  assert.ok(as("win32").includes("a:/w.sh"), as("win32"));
  assert.ok(!as("linux").includes("a:"), as("linux"));
});

test("treats .scratch/ as throwaway from a subfolder of the repository", () => {
  const cwd = join(ROOT, "scripts");
  assert.equal(exitCode({ cwd, tool_input: { file_path: join(ROOT, ".scratch", "watch.sh"), content: WATCHER } }), 2);
  assert.equal(exitCode({ cwd, tool_input: { file_path: join(ROOT, "apps", "poller.mjs"), content: WATCHER } }), 0);
});

// A regex that backtracks blocks the event loop, so each check runs in a child
// with a deadline: a regression fails in seconds rather than hanging the run.
function inTime(fn, text) {
  const code = `import * as g from ${JSON.stringify(new URL("./hooks/guard-polling.mjs", import.meta.url).href)}; import { readFileSync } from "node:fs"; process.stdout.write(JSON.stringify(g.${fn}(readFileSync(0, "utf8"), "/r")));`;
  const r = spawnSync(process.execPath, ["--input-type=module", "-e", code], { input: text, encoding: "utf8", timeout: 2000 });
  assert.equal(r.error?.code, undefined, `${fn}() did not answer within 2 seconds`);
  return JSON.parse(r.stdout);
}

test("answers at once on input built to make a pattern backtrack", () => {
  assert.equal(inTime("polls", `while x; do gh${" --a".repeat(40)}; sleep 1; done`), false);
  assert.deepEqual(inTime("scriptsIn", `bash ${"-a ".repeat(40)}x`), []);
  assert.equal(inTime("polls", `gh pr view 5${"\n".repeat(120_000)}`), false);
  assert.deepEqual(inTime("scriptsIn", "\n".repeat(120_000)), []);
});

test("blocks writing a polling script outside the repository or under .scratch/", () => {
  assert.equal(exitCode({ tool_input: { file_path: join(tmpdir(), "watch.sh"), content: WATCHER } }), 2);
  assert.equal(exitCode({ tool_input: { file_path: join(tmpdir(), "watch"), content: WATCHER } }), 2);
  assert.equal(exitCode({ tool_input: { file_path: join(ROOT, ".scratch", "watch.sh"), content: WATCHER } }), 2);
  assert.equal(exitCode({ tool_input: { file_path: join(tmpdir(), "watch.sh"), old_string: "x", new_string: WATCHER } }), 2);
  assert.equal(exitCode({ tool_input: { file_path: join(tmpdir(), "watch.sh"), edits: [{ old_string: "x", new_string: WATCHER }] } }), 2);
});

test("passes writing the repository's own scripts, which review covers, and notes in .scratch/", () => {
  assert.equal(exitCode({ tool_input: { file_path: join(ROOT, "scripts", "poller.mjs"), content: WATCHER } }), 0);
  assert.equal(exitCode({ tool_input: { file_path: join(ROOT, ".scratch", "plan.md"), content: WATCHER } }), 0);
});

test("runs on every tool that can start or write a loop", () => {
  const settings = JSON.parse(readFileSync(join(ROOT, ".claude", "settings.json"), "utf8"));
  const entry = settings.hooks.PreToolUse.find((e) => e.hooks.some((h) => h.command.includes("guard-polling.mjs")));
  assert.deepEqual(entry.matcher.split("|").sort(), ["Bash", "Edit", "Monitor", "MultiEdit", "PowerShell", "Write"]);
});

test("passes input it cannot read, and fields of an unexpected type", () => {
  assert.equal(spawnSync(process.execPath, [HOOK], { input: "not json", timeout: 10_000 }).status, 0);
  assert.equal(exitCode({ cwd: 5, tool_input: { command: "bash a.sh" } }), 0);
  assert.equal(exitCode({ tool_input: { file_path: join(tmpdir(), "w.sh"), edits: {} } }), 0);
});
