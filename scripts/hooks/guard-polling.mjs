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
// scripts, when the file is a script. A loop polls when it repeats without a fixed count (while, until,
// do, for (;;), setInterval), calls gh, and sleeps. A loop with a fixed count
// (for x in, foreach) ends, and passes.
//
// Exit 0 allows the tool call. Exit 2 blocks it and feeds stderr back to the agent.
// This is a guardrail against habit, not a security boundary.

import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// A loop keyword counts only where a command starts, so prose in a commit
// message or a pull request body ("wait while the review runs") is not a loop.
// PowerShell keywords ignore case, so its forms (a keyword then a parenthesis
// or brace) match in any case; a capitalized word that starts a sentence does
// not. Every repeated part can match a stretch of text one way only, so no
// input makes these backtrack.
const AT = String.raw`(?:^|[;&|({"'\x60]|\b(?:then|do|else)\s|-Command\s)[ \t]*`;
const LOOP = new RegExp(String.raw`${AT}(?:while|until)\b|${AT}[Ww][Hh][Ii][Ll][Ee]\s*\(|${AT}[Dd][Oo]\s*\{|${AT}[Ff][Oo][Rr]\s*\(\(?\s*;\s*;\s*\)`, "m");
// A command that repeats on its own clock needs no sleep. It counts only as
// code: a call, or watch given an interval and then gh.
const REPEAT = /\bsetInterval\s*\(|\bwatch\s+(?:-n|--interval)(?:=|\s+)\d+\s+["']?gh\b/;
const GH = /\bgh(?:\.exe)?["']?(?:\s+-{1,2}(?!-)[\w-]+(?:[ =][^\s-]\S*)?)*\s+[a-z]/i;
const SLEEP = /\b(?:sleep|Start-Sleep|setTimeout)\b|\btimeout(?:\.exe)?\s+\/t\b|\bread\s+-t\b/i;
// A script the command runs: one that starts a command, or one a shell is
// given. A script the command only names (rm, cat, pkill -f) is not read, so
// cleaning up a watcher is never blocked.
const SCRIPT = /(?:^|[;&|(\n][ \t]*|\b(?:bash|sh|zsh|pwsh|powershell)(?:\.exe)?["']?\s+(?:-[\w-]+\s+(?:[\w.][\w.-]*\s+(?=-))?)*|\b(?:nohup|setsid|exec|source|start|timeout\s+\S+|Start-Process(?:\s+-FilePath)?)\s+|(?:^|\s)[&.][ \t]+)["']?((?:[A-Za-z]:)?[^\s"';&|]*\.(?:sh|bash|ps1))(?=["'\s;&|]|$)/gm;
// The files an agent writes that can hold a loop; a note or a ticket in .scratch/ cannot.
const RUNNABLE = /\.(?:sh|bash|ps1|psm1|cmd|bat|js|mjs|cjs|ts|py)$|(?:^|[\\/])[^.\\/]+$/i;

// Whether text holds a loop that polls GitHub.
export const polls = (text) => GH.test(text) && ((LOOP.test(text) && SLEEP.test(text)) || REPEAT.test(text));

// The local shell scripts a command names, resolved against dir; on Windows a
// Git Bash drive path (/d/x.sh) is read as d:/x.sh. A network path is left
// out: opening one before the permission prompt would send the owner's
// Windows credentials to that host.
export function scriptsIn(command, dir, platform = process.platform) {
  return [...command.matchAll(SCRIPT)]
    .map((m) => resolve(dir, platform === "win32" ? m[1].replace(/^\/([a-z])\//i, "$1:/") : m[1]))
    .filter((p) => !/^[\\/]{2}/.test(p));
}

// The repository holding dir: the nearest folder up with a .git entry.
function repository(dir) {
  for (let d = resolve(dir); ; d = dirname(d)) {
    if (existsSync(join(d, ".git"))) return d;
    if (dirname(d) === d) return resolve(dir);
  }
}

// Whether a file is one an agent keeps for itself: outside the repository, or under .scratch/.
function throwaway(file, dir) {
  const root = repository(dir);
  const rel = relative(root, resolve(dir, file)).replace(/\\/g, "/");
  return rel.startsWith("../") || isAbsolute(rel) || rel.startsWith(".scratch/");
}

// A regular file's text; anything else (a pipe, a device, a large file) reads as empty.
function read(file) {
  try {
    const s = statSync(file);
    return s.isFile() && s.size < 256 * 1024 ? readFileSync(file, "utf8") : "";
  } catch {
    return "";
  }
}

// The text the tool call would run or write, as far as it polls.
function inspected(payload) {
  const input = payload?.tool_input ?? {};
  const dir = typeof payload?.cwd === "string" ? payload.cwd : process.cwd();
  if (typeof input.command === "string") return [input.command, ...scriptsIn(input.command, dir).map(read)];
  const file = input.file_path;
  if (typeof file !== "string" || !RUNNABLE.test(file) || !throwaway(file, dir)) return [];
  const edits = Array.isArray(input.edits) ? input.edits.map((e) => e?.new_string) : [];
  return [input.content, input.new_string, ...edits].filter((t) => typeof t === "string");
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
