#!/usr/bin/env node
// The fast local checks a session runs before it pushes: the same gates CI
// runs, in the order that fails cheapest first.
//
//   node scripts/check.mjs          run every check, then summarise
//   node scripts/check.mjs --quick  skip the full test run (fmt, clippy, manifest only)
//
// Exit 0 when everything passes. The manifest check may fail only because of
// open inbox entries the branch added; that is reported as a warning, since a
// planning pass clears those before merge (docs/briefs/README.md, Roles).

import { spawnSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const QUICK = process.argv.includes("--quick");

const scriptTests = existsSync(join(ROOT, "scripts"))
  ? readdirSync(join(ROOT, "scripts")).filter((f) => f.endsWith(".test.mjs")).map((f) => join("scripts", f))
  : [];

const checks = [
  { name: "rustfmt", cmd: "cargo", args: ["fmt", "--all", "--check"] },
  { name: "clippy", cmd: "cargo", args: ["clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"] },
  ...(QUICK ? [] : [{ name: "tests", cmd: "cargo", args: ["test", "--workspace", "--locked"] }]),
  ...(scriptTests.length ? [{ name: "script tests", cmd: "node", args: ["--test", ...scriptTests] }] : []),
  // The interface's TypeScript types and design tokens are generated from the
  // protocol schemas and DESIGN.md; a schema change in a Rust ticket makes them
  // stale. Needs the interface's packages installed (`pnpm install` in apps/shell).
  ...(existsSync(join(ROOT, "apps", "shell", "node_modules"))
    ? [
        { name: "interface types", cmd: "node", args: ["scripts/gen-types.mjs", "--check"], cwd: join(ROOT, "apps", "shell") },
        { name: "interface tokens", cmd: "node", args: ["scripts/gen-tokens.mjs", "--check"], cwd: join(ROOT, "apps", "shell") },
      ]
    : []),
  { name: "manifest", cmd: "node", args: ["scripts/manifest.mjs", "--check"], manifest: true },
];

const results = [];
for (const check of checks) {
  process.stdout.write(`\n== ${check.name}: ${check.cmd} ${check.args.join(" ")}\n`);
  const run = spawnSync(check.cmd, check.args, { cwd: check.cwd ?? ROOT, stdio: "inherit", shell: process.platform === "win32" });
  let status = run.status === 0 ? "pass" : "FAIL";
  if (status === "FAIL" && check.manifest && onlyInboxProblems()) status = "warn (open inbox entries only)";
  results.push({ name: check.name, status });
}

process.stdout.write("\n== summary\n");
for (const r of results) process.stdout.write(`${r.status.padEnd(32)} ${r.name}\n`);
process.exit(results.some((r) => r.status === "FAIL") ? 1 : 0);

// The manifest lists its problems under "## Problems"; inbox entries are one
// line of their own. Anything else there is a real failure.
function onlyInboxProblems() {
  const text = readFileSync(join(ROOT, "docs", "MANIFEST.md"), "utf8");
  const section = text.split("## Problems")[1] ?? "";
  const problems = section.split("\n").filter((l) => l.startsWith("- "));
  return problems.length > 0 && problems.every((l) => /open inbox entr(y|ies) need a planning pass/.test(l));
}
