#!/usr/bin/env node
// The fast local checks a session runs before it pushes: the same gates CI
// runs, in the order that fails cheapest first.
//
//   node scripts/check.mjs          run every check, then summarise
//   node scripts/check.mjs --quick  skip the full test run (fmt, clippy, manifest only)
//
// Exit 0 when everything passes.

import { spawnSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const QUICK = process.argv.includes("--quick");

const scriptTests = existsSync(join(ROOT, "scripts"))
  ? readdirSync(join(ROOT, "scripts")).filter((f) => f.endsWith(".test.mjs")).map((f) => join("scripts", f))
  : [];

const checks = [
  // `--all` formats every workspace member, the Shell included; formatting
  // needs no build, so Tauri is not compiled.
  { name: "rustfmt", cmd: "cargo", args: ["fmt", "--all", "--check"] },
  // Clippy and tests build the default members, Core's crates, as CI's linux
  // job does. CI's shell job builds the Shell (`-p ersa-shell`).
  { name: "clippy", cmd: "cargo", args: ["clippy", "--all-targets", "--locked", "--", "-D", "warnings"] },
  ...(QUICK ? [] : [{ name: "tests", cmd: "cargo", args: ["test", "--locked"] }]),
  ...(scriptTests.length ? [{ name: "script tests", cmd: "node", args: ["--test", ...scriptTests] }] : []),
  // The interface's TypeScript types, design tokens and catalog export are
  // generated from the protocol schemas, DESIGN.md and the string catalog; a
  // schema change in a Rust ticket makes them stale. Needs the interface's packages installed (`pnpm install` in apps/shell).
  ...(existsSync(join(ROOT, "apps", "shell", "node_modules"))
    ? [
        { name: "interface types", cmd: "node", args: ["scripts/gen-types.mjs", "--check"], cwd: join(ROOT, "apps", "shell") },
        { name: "interface tokens", cmd: "node", args: ["scripts/gen-tokens.mjs", "--check"], cwd: join(ROOT, "apps", "shell") },
        { name: "interface catalog export", cmd: "node", args: ["scripts/gen-catalog.mjs", "--check"], cwd: join(ROOT, "apps", "shell") },
      ]
    : []),
  { name: "manifest", cmd: "node", args: ["scripts/manifest.mjs", "--check"] },
];

const results = [];
for (const check of checks) {
  process.stdout.write(`\n== ${check.name}: ${check.cmd} ${check.args.join(" ")}\n`);
  const run = spawnSync(check.cmd, check.args, { cwd: check.cwd ?? ROOT, stdio: "inherit", shell: process.platform === "win32" });
  results.push({ name: check.name, status: run.status === 0 ? "pass" : "FAIL" });
}

process.stdout.write("\n== summary\n");
for (const r of results) process.stdout.write(`${r.status.padEnd(6)} ${r.name}\n`);
process.exit(results.some((r) => r.status === "FAIL") ? 1 : 0);
