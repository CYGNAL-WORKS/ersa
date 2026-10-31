#!/usr/bin/env node
// Declares this checkout's session role for the brief-guard hook.
//
//   node scripts/role.mjs planning       allow edits to docs/briefs and docs/adr
//   node scripts/role.mjs implementing   block them (the default when no role file exists)
//   node scripts/role.mjs                print the current role
//
// Writes .claude/role.local, which is not committed. A fresh checkout or worktree
// therefore starts as implementing. ERSA_ROLE in the environment overrides the file.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const FILE = join(ROOT, ".claude", "role.local");
const arg = process.argv[2];

if (!arg) {
  const fromEnv = process.env.ERSA_ROLE;
  const fromFile = existsSync(FILE) ? readFileSync(FILE, "utf8").trim() : "";
  console.log(fromEnv || fromFile || "implementing (default)");
  process.exit(0);
}

if (arg !== "planning" && arg !== "implementing") {
  console.error("usage: node scripts/role.mjs [planning|implementing]");
  process.exit(1);
}

mkdirSync(join(ROOT, ".claude"), { recursive: true });
writeFileSync(FILE, arg + "\n");
console.log(`role set to ${arg} (${FILE})`);
