#!/usr/bin/env node
// Claude Code PreToolUse hook: blocks a loop that polls GitHub with gh. Every
// gh call spends the owner's GitHub budget, which the merge review poller,
// every session and the app share. A Monitor that expires, or a background
// command that is stopped, leaves its script running on Windows. A session
// that re-armed a watcher every half hour left dozens polling for good.
//
// It reads the Bash, PowerShell or Monitor command, and the shell scripts
// that command runs. It also reads what a Write or Edit puts in a file
// outside the repository or under .scratch/, where agents keep throwaway
// scripts. A loop polls when it repeats without a fixed count (while, until,
// do, for (;;), setInterval), calls gh, and sleeps. A loop with a fixed count
// (for x in, foreach) ends, and passes.
//
// Exit 0 allows the tool call. Exit 2 blocks it and feeds stderr back to the agent.
// This is a guardrail against habit, not a security boundary.

import { existsSync, readFileSync, statSync } from "node:fs";
import { isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const LOOP = /\b(while|until)\b|\bdo\s*\{|\bfor\s*\(\s*;\s*;\s*\)|\bsetInterval\b/;
const GH = /\bgh(?:\.exe)?["']?\s+(?:pr|run|api|issue|workflow|repo|release|search|label|status)\b/i;
const SLEEP = /\b(?:sleep|Start-Sleep|setTimeout)\b|\btimeout(?:\.exe)?\s+\/t\b/i;
const SCRIPT = /(?:^|[\s"'])((?:[A-Za-z]:)?[^\s"';&|]*\.(?:sh|bash|ps1))(?=["'\s;&|]|$)/g;

// Whether text holds a loop that polls GitHub.
export const polls = (text) => LOOP.test(text) && GH.test(text) && SLEEP.test(text);

// The shell scripts a command names, resolved against dir.
export function scriptsIn(command, dir) {
  return [...command.matchAll(SCRIPT)].map((m) => resolve(dir, m[1].replace(/^\/([a-z])\//i, "$1:/")));
}

// Whether a file is one an agent keeps for itself: outside the repository, or under .scratch/.
export function throwaway(file, root) {
  const rel = relative(root, resolve(root, file)).replace(/\\/g, "/");
  return rel.startsWith("../") || isAbsolute(rel) || rel.startsWith(".scratch/");
}

function read(file) {
  try {
    return existsSync(file) && statSync(file).size < 256 * 1024 ? readFileSync(file, "utf8") : "";
  } catch {
    return "";
  }
}

// The text the tool call would run or write, as far as it polls.
export function inspected(payload) {
  const input = payload?.tool_input ?? {};
  const dir = payload?.cwd ?? process.cwd();
  if (typeof input.command === "string") return [input.command, ...scriptsIn(input.command, dir).map(read)];
  const file = input.file_path ?? input.notebook_path;
  if (typeof file !== "string" || !throwaway(file, dir)) return [];
  return [input.content, input.new_string, ...(input.edits ?? []).map((e) => e?.new_string)].filter((t) => typeof t === "string");
}

function main() {
  let payload;
  try {
    payload = JSON.parse(readFileSync(0, "utf8"));
  } catch {
    process.exit(0);
  }
  if (!inspected(payload).some(polls)) process.exit(0);
  process.stderr.write(
    "Blocked: this loop polls GitHub with gh. Every gh call spends the owner's shared GitHub budget, " +
      "and on Windows a stopped command or expired Monitor leaves the loop running.\n" +
      "To wait on a pull request's checks, run `gh pr checks <n> --watch`, which exits when they finish. " +
      "An orchestrating session learns of reviews and merges from the implementing session's report (docs/agents/orchestration.md).\n"
  );
  process.exit(2);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
