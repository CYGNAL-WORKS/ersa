#!/usr/bin/env node
// Records a BIDIRECTIONAL Claude Code streaming-JSON session as a protocol fixture.
//
//   node scripts/record-claude-stream.mjs <name> "<message 1>" ["<message 2>" ...] [-- extra claude flags...]
//
// Starts the CLI with --input-format stream-json and --output-format stream-json,
// sends each message as a stream-json user message, waits for the matching
// `result` line before sending the next, then closes stdin and waits for exit.
// Any control_request that arrives (for example a permission prompt) is answered
// with an allow so the turn can complete; both the request and our response are
// recorded. Writes the same .jsonl/.meta.json pair as record-claude.mjs, with the
// lines we SENT recorded in the meta file under `sent`.
//
// Both files pass through redact() (scripts/redact.mjs) before they are written:
// account email, organization, home-directory and repository paths become
// placeholders. An --mcp-config file among the extra flags names the
// repository by that placeholder; the CLI gets an expanded copy.
//
// Set ERSA_CLAUDE to the claude executable if it is not on PATH.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expandMcpConfig, redact } from "./redact.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const CLAUDE = process.env.ERSA_CLAUDE || "claude";
const argv = process.argv.slice(2);
const sep = argv.indexOf("--");
const positional = sep === -1 ? argv : argv.slice(0, sep);
const extra = sep === -1 ? [] : argv.slice(sep + 1);
const [name, ...messages] = positional;

if (!name || messages.length === 0) {
  console.error('usage: node scripts/record-claude-stream.mjs <name> "<message>" [...] [-- extra flags]');
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

const cwd = mkdtempSync(join(tmpdir(), "ersa-rec-"));
expandMcpConfig(extra, cwd);

const args = [
  "--input-format", "stream-json",
  "--output-format", "stream-json",
  "--verbose",
  "--permission-mode", "default",
  "--setting-sources", "",
  "--strict-mcp-config",
  ...extra,
];

const started = Date.now();
const child = spawn(CLAUDE, args, { cwd, stdio: ["pipe", "pipe", "pipe"] });

const stdoutLines = [];
const sent = [];
let stderr = "";
let buffer = "";
let turn = 0;
let resolveResult = null;

function send(obj) {
  const line = JSON.stringify(obj);
  sent.push({ t: Date.now() - started, line });
  child.stdin.write(line + "\n");
}

function userMessage(text) {
  return { type: "user", message: { role: "user", content: [{ type: "text", text }] } };
}

child.stderr.on("data", (d) => (stderr += d.toString()));
child.stdout.on("data", (d) => {
  buffer += d.toString();
  let i;
  while ((i = buffer.indexOf("\n")) !== -1) {
    const line = buffer.slice(0, i).replace(/\r$/, "");
    buffer = buffer.slice(i + 1);
    if (!line) continue;
    stdoutLines.push({ t: Date.now() - started, line });
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      continue;
    }
    if (msg.type === "control_request") {
      // Allow whatever is asked so the turn completes; the shapes are what we are recording.
      const req = msg.request ?? {};
      const response = { behavior: "allow" };
      if (req.input !== undefined) response.updatedInput = req.input;
      send({ type: "control_response", response: { subtype: "success", request_id: msg.request_id, response } });
    }
    if (msg.type === "result" && resolveResult) {
      const r = resolveResult;
      resolveResult = null;
      r(msg);
    }
  }
});

const exited = new Promise((resolve) => child.on("exit", (code, signal) => resolve({ code, signal })));

function waitForResult(ms) {
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      resolveResult = null;
      resolve(null);
    }, ms);
    resolveResult = (msg) => {
      clearTimeout(timer);
      resolve(msg);
    };
  });
}

// Optional preamble: ERSA_STREAM_INIT=1 sends an SDK-style initialize control request
// before the first user message, so the CLI treats us as a host that can answer
// can_use_tool requests. We record whatever comes back.
if (process.env.ERSA_STREAM_INIT) {
  send({
    type: "control_request",
    request_id: "ersa-init-1",
    request: { subtype: "initialize", hooks: {} },
  });
  await new Promise((r) => setTimeout(r, 1500));
}

const turns = [];
for (const text of messages) {
  turn += 1;
  const p = waitForResult(180_000);
  send(userMessage(text));
  const res = await p;
  turns.push({ turn, text, got_result: !!res, subtype: res?.subtype ?? null, is_error: res?.is_error ?? null });
  if (!res) break;
}
child.stdin.end();
const exit = await Promise.race([exited, new Promise((r) => setTimeout(() => r({ code: null, signal: "timeout" }), 30_000))]);
if (exit.signal === "timeout") child.kill();
const elapsed_ms = Date.now() - started;

writeFileSync(join(dir, stem + ".jsonl"), redact(stdoutLines.map((l) => l.line).join("\n") + "\n"));
writeFileSync(
  join(dir, stem + ".meta.json"),
  redact(JSON.stringify(
    {
      recorded_at: new Date(started).toISOString(),
      claude_code_version: version,
      executable: CLAUDE,
      args,
      bidirectional: true,
      cwd_was_empty_tempdir: true,
      exit_code: exit.code,
      signal: exit.signal,
      elapsed_ms,
      stderr,
      stdout_lines: stdoutLines.length,
      stdout_timing_ms: stdoutLines.map((l) => l.t),
      sent,
      turns,
    },
    null,
    2
  ) + "\n")
);

console.log(`recorded ${stem} (${version}): exit ${exit.code ?? exit.signal}, ${stdoutLines.length} lines out, ${sent.length} lines in, ${elapsed_ms} ms`);
for (const t of turns) console.log(`  turn ${t.turn}: ${t.got_result ? t.subtype : "NO RESULT"}${t.is_error ? " (error)" : ""}`);
if (stderr) console.log("stderr:", stderr.trim().slice(0, 400));
