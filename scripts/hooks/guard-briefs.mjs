#!/usr/bin/env node
// Claude Code PreToolUse hook: blocks file edits under docs/briefs/ and docs/adr/
// unless the session has declared the planning role, either with ERSA_ROLE=planning
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

const guarded = /(^|\/)docs\/(briefs|adr)\//.test(target);
// Always writable: the inbox (findings) and recorded protocol fixtures (evidence).
const inbox = /(^|\/)docs\/briefs\/inbox\.md$/.test(target) || /(^|\/)docs\/briefs\/protocol\/fixtures\//.test(target);

function role() {
  if (process.env.ERSA_ROLE) return process.env.ERSA_ROLE;
  if (existsSync(ROLE_FILE)) return readFileSync(ROLE_FILE, "utf8").trim();
  return "implementing";
}

if (guarded && !inbox && role() !== "planning") {
  process.stderr.write(
    "Blocked: briefs and ADRs are edited only in planning sessions. " +
      "Planning sessions declare themselves with `node scripts/role.mjs planning`. " +
      "If a brief contradicts reality or a decision is missing, append a line to docs/briefs/INBOX.md and continue.\n"
  );
  process.exit(2);
}

process.exit(0);
