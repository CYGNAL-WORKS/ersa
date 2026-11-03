#!/usr/bin/env node
// Claude Code PreToolUse hook: runs `node scripts/manifest.mjs --check` before an
// agent's `git commit`, so no commit carries a structural problem CI would fail
// on, or a docs/MANIFEST.md or ADR index older than the briefs beside it.
//
// It blocks when the check reports a problem other than open inbox entries
// (implementing sessions commit with the entries they added; a planning pass
// clears them before merge), and when the check rewrote a generated file the
// commit would leave out. The repository is the one `git -C <dir>` names, else
// the session's working directory.
//
// Exit 0 allows the tool call. Exit 2 blocks it and feeds stderr back to the agent.
// This is a guardrail against habit, not a security boundary.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const GENERATED = ["docs/MANIFEST.md", "docs/adr/README.md"];

// `git [-C <dir>] [-c <k=v>] commit <args>` anywhere in a compound command.
// Returns null for anything else, otherwise the directory `-C` names (or null)
// and whether the commit stages tracked changes itself (`-a`, `--all`).
export function commitTarget(command) {
  const match = /\bgit((?:\s+-[Cc]\s+(?:"[^"]*"|'[^']*'|[^\s;&|]+))*)\s+commit(?![\w-])([^;&|\n]*)/.exec(command);
  if (!match) return null;
  const dirArg = /-C\s+("[^"]*"|'[^']*'|[^\s;&|]+)/.exec(match[1]);
  const dir = dirArg ? dirArg[1].replace(/^["']|["']$/g, "") : null;
  const args = match[2].replace(/"[^"]*"|'[^']*'/g, "");
  const all = /(^|\s)(-(?!-)[A-Za-z]*a[A-Za-z]*|--all)(?=\s|$)/.test(args);
  return { dir, all };
}

// The manifest lists its problems under "## Problems"; open inbox entries are
// one line of their own (as scripts/check.mjs reads it).
export function blockingProblems(manifest) {
  const section = manifest.split("## Problems")[1] ?? "";
  return section
    .split(/\r?\n/)
    .filter((l) => l.startsWith("- "))
    .filter((l) => !/open inbox entr(y|ies) need a planning pass/.test(l));
}

function git(dir, args) {
  return spawnSync("git", ["-C", dir, ...args], { encoding: "utf8" });
}

function block(message) {
  process.stderr.write(message + "\n");
  process.exit(2);
}

function main() {
  let payload;
  try {
    payload = JSON.parse(readFileSync(0, "utf8"));
  } catch {
    process.exit(0);
  }
  const target = commitTarget(String(payload?.tool_input?.command ?? ""));
  if (!target) process.exit(0);

  const base = payload?.cwd ?? process.cwd();
  const top = git(target.dir ? resolve(base, target.dir) : base, ["rev-parse", "--show-toplevel"]);
  if (top.status !== 0) process.exit(0);
  const root = top.stdout.trim();
  const script = join(root, "scripts", "manifest.mjs");
  if (!existsSync(script)) process.exit(0);

  const run = spawnSync(process.execPath, [script, "--check"], { cwd: root, encoding: "utf8" });
  if (run.status !== 0) {
    const manifest = join(root, "docs", "MANIFEST.md");
    const problems = existsSync(manifest) ? blockingProblems(readFileSync(manifest, "utf8")) : [];
    if (problems.length || !existsSync(manifest)) {
      block(
        "Blocked: the manifest check failed, and CI runs the same check.\n" +
          (problems.length ? problems.join("\n") : (run.stdout + run.stderr).trim()) +
          "\nFix these, then commit again."
      );
    }
  }

  if (!target.all) {
    const stale = git(root, ["diff", "--name-only", "--", ...GENERATED]).stdout.trim();
    if (stale) {
      block(
        `Blocked: the manifest check regenerated files this commit would leave out:\n${stale}\n` +
          "Stage them, then commit again."
      );
    }
  }
  process.exit(0);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
