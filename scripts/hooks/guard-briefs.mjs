#!/usr/bin/env node
// Claude Code PreToolUse hook: blocks edits to planning-only paths (docs/briefs/,
// docs/adr/, docs/GLOSSARY.md, CLAUDE.md, .claude/skills/, .claude/settings.json,
// scripts/hooks/) unless the session has declared the planning role, either with ERSA_ROLE=planning
// in the environment or by running `node scripts/role.mjs planning`, which writes
// the uncommitted file .claude/role.local. The inbox is always writable so
// implementing sessions can record findings.
//
// Exit 0 allows the tool call. Exit 2 blocks it and feeds stderr back to the agent.
// This is a guardrail against habit, not a security boundary.

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..", "..");
const ROLE_FILE = join(ROOT, ".claude", "role.local");

let input = "";
try {
  input = readFileSync(0, "utf8");
} catch {
  process.exit(0);
}

let payload;
try {
  payload = JSON.parse(input);
} catch {
  process.exit(0);
}

const target = String(
  payload?.tool_input?.file_path ?? payload?.tool_input?.notebook_path ?? payload?.tool_input?.path ?? ""
)
  .replace(/\\/g, "/")
  .toLowerCase();

if (!target) process.exit(0);

// Planning-only paths: the specification, the decisions, the vocabulary, the
// agent instructions, and this guard itself.
const guarded =
  /(^|\/)docs\/(briefs|adr)\//.test(target) ||
  /(^|\/)docs\/glossary\.md$/.test(target) ||
  /(^|\/)claude\.md$/.test(target) ||
  /(^|\/)\.claude\/(skills\/|settings\.json$)/.test(target) ||
  /(^|\/)scripts\/hooks\//.test(target);
// Always writable: the inbox (findings) and recorded protocol fixtures (evidence).
const inbox = /(^|\/)docs\/briefs\/inbox\.md$/.test(target) || /(^|\/)docs\/briefs\/protocol\/fixtures\//.test(target);

function role() {
  if (process.env.ERSA_ROLE) return process.env.ERSA_ROLE;
  if (existsSync(ROLE_FILE)) return readFileSync(ROLE_FILE, "utf8").trim();
  return "implementing";
}

if (guarded && !inbox && role() !== "planning") {
  process.stderr.write(
    "Blocked: briefs, ADRs, the glossary, CLAUDE.md, .claude/skills, .claude/settings.json and scripts/hooks " +
      "are edited only in planning sessions. Record what should change as one line in docs/briefs/INBOX.md and continue.\n"
  );
  process.exit(2);
}

process.exit(0);
