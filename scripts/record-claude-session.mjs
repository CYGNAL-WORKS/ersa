#!/usr/bin/env node
// Records a Claude Code session the way Ersa runs one, as a protocol fixture
// pair: the live stream and the CLI's own session record for the same run.
//
//   node scripts/record-claude-session.mjs <name> [--cwd <dir>] [--resume <session id>] "<message 1>" ["<message 2>" ...]
//
// Starts the CLI with Ersa's command line and environment (PROTO-CLAUDE-CODE-STREAM,
// Invocation), sends `initialize` and waits for its answer, sends each message
// and waits for its `result`, allows every permission prompt, then closes stdin
// and waits for the exit. Then it copies the session record the CLI kept,
// `~/.claude/projects/<encoded cwd>/<session id>.jsonl`, as it stood after the run.
//
// Without --cwd it works in a new scratch git repository under the temp
// directory, never a real project, and prints its path so a later run can
// resume there with --cwd and --resume. Never point --cwd at a real project:
// the CLI loads the user's settings, and every tool call is allowed. Writes, under
// docs/briefs/protocol/fixtures/claude-code/<version>/:
//   <NN>-<name>.jsonl          the CLI's stdout
//   <NN>-<name>.meta.json      command line, environment changes, stdin lines, timings
//   record/<NN>-<name>.jsonl   the session record after the run
//
// Everything passes through redact() (scripts/redact.mjs) before it is written,
// and the OS user name is replaced wherever it appears, since streamed tool
// input can split a path mid-name. The record keeps verbatim the entries the
// adapter reads (READ_ENTRIES); every other entry is cut to its kind and marked
// `"ersa_elided": true`, because those bodies are internal to the CLI and carry
// its system prompt and the recording machine's configuration.
// Set ERSA_CLAUDE to the claude executable if it is not on PATH.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir, tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { redact } from "./redact.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const CLAUDE = process.env.ERSA_CLAUDE || "claude";

// Ersa's command line (PROTO-CLAUDE-CODE-STREAM, Invocation).
const ERSA_ARGS = [
  "-p",
  "--input-format", "stream-json",
  "--output-format", "stream-json",
  "--verbose",
  "--include-partial-messages",
  "--permission-mode", "default",
  "--setting-sources", "user,project,local",
  "--permission-prompt-tool", "stdio",
  "--await-initialize",
];
// Ersa's environment changes: its own entrypoint, and nothing inherited from
// whatever started it.
const ENV_SET = { CLAUDE_CODE_ENTRYPOINT: "ersa" };
const ENV_REMOVED = ["CLAUDECODE", "CLAUDE_CODE_CHILD_SESSION", "NODE_OPTIONS", "TRACEPARENT", "TRACESTATE"];

// The session record entries the adapter reads (PROTO-CLAUDE-CODE-STREAM, Session record).
const READ_ENTRIES = new Set(["user", "assistant", "custom-title", "ai-title", "last-prompt"]);

const USER_NAME = userInfo().username;
function scrub(text) {
  const redacted = redact(text);
  if (USER_NAME.length < 3) return redacted;
  const escaped = USER_NAME.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return redacted.replace(new RegExp(`\\b${escaped}\\b`, "gi"), "user");
}

function elide(recordText) {
  return recordText
    .split("\n")
    .map((line) => {
      if (!line.trim()) return line;
      let entry;
      try {
        entry = JSON.parse(line);
      } catch {
        return line;
      }
      if (READ_ENTRIES.has(entry.type)) return line;
      const kept = { type: entry.type };
      if (entry.subtype !== undefined) kept.subtype = entry.subtype;
      if (entry.attachment?.type !== undefined) kept.attachment = { type: entry.attachment.type };
      kept.ersa_elided = true;
      return JSON.stringify(kept);
    })
    .join("\n");
}

const argv = process.argv.slice(2);
const option = (flag) => {
  const i = argv.indexOf(flag);
  if (i === -1) return undefined;
  const [, value] = argv.splice(i, 2);
  return value;
};
const givenCwd = option("--cwd");
const resume = option("--resume");
const [name, ...messages] = argv;
if (!name || messages.length === 0 || !/^[a-z0-9-]+$/.test(name)) {
  console.error('usage: node scripts/record-claude-session.mjs <name> [--cwd <dir>] [--resume <id>] "<message>" [...]');
  process.exit(1);
}

const version = spawnSync(CLAUDE, ["--version"], { encoding: "utf8" }).stdout?.trim().split(/\s+/)[0];
if (!version) {
  console.error(`could not run ${CLAUDE} --version`);
  process.exit(1);
}

const dir = join(ROOT, "docs", "briefs", "protocol", "fixtures", "claude-code", version);
mkdirSync(join(dir, "record"), { recursive: true });
const existing = readdirSync(dir).filter((f) => /^\d{2}-.*\.jsonl$/.test(f)).length;
const stem = `${String(existing).padStart(2, "0")}-${name}`;
if (existsSync(join(dir, stem + ".jsonl"))) {
  console.error(`fixture ${stem} already exists`);
  process.exit(1);
}

function scratchRepo() {
  const repo = mkdtempSync(join(tmpdir(), "ersa-rec-"));
  const git = (...args) => spawnSync("git", args, { cwd: repo, encoding: "utf8" });
  git("init", "--quiet");
  // The CLI quotes the git user in its prompt; keep the owner's out of it.
  git("config", "user.name", "Ersa recorder");
  git("config", "user.email", "recorder@example.com");
  writeFileSync(join(repo, "README.md"), "Scratch repository for an Ersa protocol recording.\n");
  git("add", "README.md");
  git("commit", "--quiet", "-m", "Scratch");
  return repo;
}
const cwd = givenCwd ?? scratchRepo();

const args = [...ERSA_ARGS, ...(resume ? ["--resume", resume] : [])];
const env = { ...process.env, ...ENV_SET };
for (const key of ENV_REMOVED) delete env[key];

const started = Date.now();
const child = spawn(CLAUDE, args, { cwd, env, stdio: ["pipe", "pipe", "pipe"] });

const stdoutLines = [];
const sent = [];
let stderr = "";
let buffer = "";
let waiting = null; // { want: (msg) => bool, resolve }

function send(obj) {
  const line = JSON.stringify(obj);
  sent.push({ t: Date.now() - started, line });
  child.stdin.write(line + "\n");
}

function waitFor(want, ms) {
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      waiting = null;
      resolve(null);
    }, ms);
    waiting = {
      want,
      resolve: (msg) => {
        clearTimeout(timer);
        waiting = null;
        resolve(msg);
      },
    };
  });
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
    if (msg.type === "control_request" && msg.request?.subtype === "can_use_tool") {
      // Allow with the input as asked, the way Ersa answers an unedited allow.
      send({
        type: "control_response",
        response: {
          subtype: "success",
          request_id: msg.request_id,
          response: { behavior: "allow", updatedInput: msg.request.input },
        },
      });
    }
    if (waiting?.want(msg)) waiting.resolve(msg);
  }
});

const exited = new Promise((resolve) => child.on("exit", (code, signal) => resolve({ code, signal })));

const initAnswer = waitFor((m) => m.type === "control_response" && m.response?.request_id === "ersa-init-1", 60_000);
send({ type: "control_request", request_id: "ersa-init-1", request: { subtype: "initialize", hooks: {} } });
const initialized = !!(await initAnswer);

const turns = [];
if (initialized) {
  for (const [n, text] of messages.entries()) {
    const result = waitFor((m) => m.type === "result", 300_000);
    send({ type: "user", message: { role: "user", content: [{ type: "text", text }] } });
    const res = await result;
    turns.push({ turn: n + 1, text, got_result: !!res, subtype: res?.subtype ?? null, is_error: res?.is_error ?? null });
    if (!res) break;
  }
}
child.stdin.end();
const exit = await Promise.race([exited, new Promise((r) => setTimeout(() => r({ code: null, signal: "timeout" }), 30_000))]);
if (exit.signal === "timeout") child.kill();
const elapsed_ms = Date.now() - started;

// The session record: the init line names the session and the directory the
// CLI keyed it by.
const init = stdoutLines
  .map((l) => {
    try {
      return JSON.parse(l.line);
    } catch {
      return null;
    }
  })
  .find((m) => m?.type === "system" && m?.subtype === "init");
let recordLines = null;
if (init) {
  const encoded = init.cwd.replace(/[^a-zA-Z0-9]/g, "-");
  const recordPath = join(homedir(), ".claude", "projects", encoded, `${init.session_id}.jsonl`);
  if (existsSync(recordPath)) recordLines = readFileSync(recordPath, "utf8");
}

writeFileSync(join(dir, stem + ".jsonl"), scrub(stdoutLines.map((l) => l.line).join("\n") + "\n"));
if (recordLines !== null) writeFileSync(join(dir, "record", stem + ".jsonl"), scrub(elide(recordLines)));
writeFileSync(
  join(dir, stem + ".meta.json"),
  scrub(JSON.stringify(
    {
      recorded_at: new Date(started).toISOString(),
      claude_code_version: version,
      executable: CLAUDE,
      args,
      env_set: ENV_SET,
      env_removed: ENV_REMOVED,
      bidirectional: true,
      cwd_was_scratch_git_repo: givenCwd === undefined,
      session_id: init?.session_id ?? null,
      session_record: recordLines === null ? null : `record/${stem}.jsonl`,
      session_record_verbatim_entries: [...READ_ENTRIES],
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

// The scrub matches the user name as a word; say so if it survives elsewhere.
const written = [join(dir, stem + ".jsonl"), join(dir, stem + ".meta.json"), join(dir, "record", stem + ".jsonl")];
for (const file of written.filter(existsSync)) {
  if (USER_NAME.length >= 3 && readFileSync(file, "utf8").toLowerCase().includes(USER_NAME.toLowerCase())) {
    console.error(`WARNING: ${file} still contains the user name; redact it before committing`);
  }
}
console.log(`recorded ${stem} (${version}) in ${cwd}`);
console.log(`  session ${init?.session_id ?? "?"}: exit ${exit.code ?? exit.signal}, ${stdoutLines.length} lines out, ${sent.length} in, ${elapsed_ms} ms, record ${recordLines === null ? "NOT FOUND" : "copied"}`);
for (const t of turns) console.log(`  turn ${t.turn}: ${t.got_result ? t.subtype : "NO RESULT"}${t.is_error ? " (error)" : ""}`);
if (stderr) console.log("stderr:", stderr.trim().slice(0, 400));
