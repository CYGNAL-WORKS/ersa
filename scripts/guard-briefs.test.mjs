// Tests for hooks/guard-briefs.mjs: which paths an implementing session may edit.
// Run: node --test scripts/guard-briefs.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const HOOK = join(fileURLToPath(import.meta.url), "..", "hooks", "guard-briefs.mjs");

function exitCode(filePath, role) {
  const env = { ...process.env, ERSA_ROLE: role };
  const r = spawnSync(process.execPath, [HOOK], { input: JSON.stringify({ tool_input: { file_path: filePath } }), env });
  return r.status;
}

test("blocks planning-only paths in an implementing session", () => {
  for (const p of [
    ".claude/agents/a.md",
    "D:\\repo\\.claude\\agents\\spec-reviewer.md",
    ".claude/skills/implement-ticket/SKILL.md",
    ".claude/settings.json",
    "docs/briefs/FLOW-X.md",
    "docs/adr/0001-x.md",
    "docs/GLOSSARY.md",
    "CLAUDE.md",
    "REVIEW.md",
    ".claude/review.json",
    "scripts/hooks/guard-briefs.mjs",
  ]) {
    assert.equal(exitCode(p, "implementing"), 2, p);
  }
});

test("allows the inbox, fixtures and ordinary paths in an implementing session", () => {
  for (const p of ["docs/briefs/INBOX.md", "docs/briefs/protocol/fixtures/a.jsonl", "scripts/check.mjs", ".claude/settings.local.json"]) {
    assert.equal(exitCode(p, "implementing"), 0, p);
  }
});

test("allows planning-only paths in a planning session", () => {
  assert.equal(exitCode(".claude/agents/a.md", "planning"), 0);
});

// The hook as Claude Code runs it: the command .claude/settings.json gives,
// through a shell whose folder is a subfolder (a `cd` earlier in the session).
test("blocks from a subfolder of the project, as the settings command runs it", () => {
  const root = join(fileURLToPath(import.meta.url), "..", "..");
  const settings = JSON.parse(readFileSync(join(root, ".claude", "settings.json"), "utf8"));
  const { command } = settings.hooks.PreToolUse.find((h) => h.matcher.includes("Edit")).hooks[0];
  const r = spawnSync("bash", ["-c", command], {
    cwd: join(root, "docs"),
    input: JSON.stringify({ tool_input: { file_path: join(root, "docs", "adr", "0001-x.md") } }),
    env: { ...process.env, ERSA_ROLE: "implementing", CLAUDE_PROJECT_DIR: root },
    encoding: "utf8",
  });
  assert.equal(r.status, 2, r.stderr);
});
