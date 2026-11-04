#!/usr/bin/env node
// Claude Code PreToolUse hook: runs `node scripts/manifest.mjs --check` before an
// agent's `git commit`, so no commit carries a structural problem CI would fail
// on, or a docs/MANIFEST.md or ADR index older than the briefs beside it.
//
// It blocks when the check reports a problem other than open inbox entries
// (implementing sessions commit with the entries they added; a planning pass
// clears them before merge), and when the check rewrote a generated file the
// commit would leave out. The repository is the one `git -C <dir>` names, else
// the session's working directory, and the check runs only when that is the
// session's own repository (any of its worktrees): the hook runs before the
// user's permission prompt, so it must not run another repository's script.
// A commit after a directory change (`cd x && git commit`) runs no check,
// since where it lands is unknown; CI checks it.
//
// Exit 0 allows the tool call. Exit 2 blocks it and feeds stderr back to the agent.
// This is a guardrail against habit, not a security boundary.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const GENERATED = ["docs/MANIFEST.md", "docs/adr/README.md"];

// `git [-C <dir>] [-c <k=v>] commit <args>` anywhere in a compound command.
// Returns null for anything else, and for a commit after a directory change in
// the same command. Otherwise the directory `-C` names (or null)
// and whether the commit stages tracked changes itself (`-a`, `--all`).
export function commitTarget(command) {
  const match = /\bgit((?:\s+-[Cc]\s+(?:"[^"]*"|'[^']*'|[^\s;&|]+))*)\s+commit(?![\w-])([^;&|\n]*)/.exec(command);
  if (!match) return null;
  if (/(^|[\s;&|({])(cd|chdir|pushd|popd|sl|set-location|push-location|pop-location)(?=[\s;&|)}]|$)/i.test(command.slice(0, match.index))) {
    return null;
  }
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
  return spawnSync("git", ["-C", dir, ...args], { encoding: "utf8", timeout: 10_000 });
}

// The repository's shared git directory, the same for all its worktrees.
function commonDir(dir) {
  const run = git(dir, ["rev-parse", "--path-format=absolute", "--git-common-dir"]);
  if (run.status !== 0) return null;
  const path = resolve(run.stdout.trim());
  return process.platform === "win32" ? path.toLowerCase() : path;
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
  const dir = target.dir ? resolve(base, target.dir) : base;
  const own = commonDir(base);
  if (!own || commonDir(dir) !== own) process.exit(0);
  const top = git(dir, ["rev-parse", "--show-toplevel"]);
  if (top.status !== 0) process.exit(0);
  const root = top.stdout.trim();
  const script = join(root, "scripts", "manifest.mjs");
  if (!existsSync(script)) process.exit(0);

  const run = spawnSync(process.execPath, [script, "--check"], { cwd: root, encoding: "utf8", timeout: 120_000 });
  if (run.status !== 0) {
    // The manifest on disk is this run's only when the script finished: it
    // exits 1 after saying it wrote the file. Anything else (a crash, a signal,
    // a timeout) blocks with the script's output.
    const wrote = run.status === 1 && /^wrote docs.MANIFEST.md/m.test(run.stdout ?? "");
    const manifest = join(root, "docs", "MANIFEST.md");
    const text = wrote && existsSync(manifest) ? readFileSync(manifest, "utf8") : "";
    const problems = blockingProblems(text);
    const onlyInbox = /open inbox entr(y|ies) need a planning pass/.test(text);
    if (problems.length || !onlyInbox) {
      block(
        "Blocked: the manifest check failed, and CI runs the same check.\n" +
          (problems.length ? problems.join("\n") : `${run.stdout ?? ""}${run.stderr ?? ""}${run.error ?? ""}`.trim()) +
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
