#!/usr/bin/env node
// Records a Claude Code streaming-JSON session as a protocol fixture.
//
//   node scripts/record-claude.mjs <name> "<prompt>" [extra claude flags...]
//
// Runs the CLI in print mode with stream-json output, every behaviour-affecting
// flag passed explicitly, in an empty scratch directory, and writes:
//   docs/briefs/protocol/fixtures/claude-code/<version>/<NN>-<name>.jsonl   raw stdout, redacted
//   docs/briefs/protocol/fixtures/claude-code/<version>/<NN>-<name>.meta.json  command line, exit code, stderr, timings
//
// Both files pass through redact() (scripts/redact.mjs) before they are written:
// account email, organization, home-directory and repository paths become
// placeholders. An --mcp-config file among the extra flags names the
// repository by that placeholder; the CLI gets an expanded copy.
//
// Set ERSA_CLAUDE to the claude executable if it is not on PATH.
// Fixtures are the evidence behind PROTO-CLAUDE-CODE-STREAM; never hand-edit them.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expandMcpConfig, redact } from "./redact.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const CLAUDE = process.env.ERSA_CLAUDE || "claude";
const [name, prompt, ...extra] = process.argv.slice(2);

if (!name || !prompt) {
  console.error('usage: node scripts/record-claude.mjs <name> "<prompt>" [extra claude flags...]');
  process.exit(1);
}

const version = spawnSync(CLAUDE, ["--version"], { encoding: "utf8" }).stdout?.trim().split(/\s+/)[0];
if (!version) {
  console.error(`could not run ${CLAUDE} --version`);
  process.exit(1);
}

const dir = join(ROOT, "docs", "briefs", "protocol", "fixtures", "claude-code", version);
mkdirSync(dir, { recursive: true });
const existing = readdirSync(dir).filter((f) => /^\d{2}-.*\.jsonl$/.test(f)).length;
const stem = `${String(existing).padStart(2, "0")}-${name}`;
if (existsSync(join(dir, stem + ".jsonl"))) {
  console.error(`fixture ${stem} already exists`);
  process.exit(1);
}

// Explicit flags: nothing here relies on a CLI default (ADR 0012).
const args = [
  "-p",
  "--output-format", "stream-json",
  "--verbose",
  "--permission-mode", "default",
  "--setting-sources", "",
  "--strict-mcp-config",
  ...extra,
  prompt,
];

const cwd = mkdtempSync(join(tmpdir(), "ersa-rec-"));
const started = Date.now();
const res = spawnSync(CLAUDE, expandMcpConfig(args), { cwd, encoding: "utf8", input: "", maxBuffer: 64 * 1024 * 1024, timeout: 180_000 });
const elapsed_ms = Date.now() - started;

writeFileSync(join(dir, stem + ".jsonl"), redact(res.stdout ?? ""));
writeFileSync(
  join(dir, stem + ".meta.json"),
  redact(JSON.stringify(
    {
      recorded_at: new Date(started).toISOString(),
      claude_code_version: version,
      executable: CLAUDE,
      args,
      cwd_was_empty_tempdir: true,
      exit_code: res.status,
      signal: res.signal,
      elapsed_ms,
      stderr: res.stderr ?? "",
      stdout_lines: (res.stdout ?? "").split(/\r?\n/).filter(Boolean).length,
    },
    null,
    2
  ) + "\n")
);

console.log(`recorded ${stem} (${version}): exit ${res.status}, ${(res.stdout ?? "").split(/\r?\n/).filter(Boolean).length} lines, ${elapsed_ms} ms`);
if (res.stderr) console.log("stderr:", res.stderr.trim().slice(0, 400));
