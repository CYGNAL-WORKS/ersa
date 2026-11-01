#!/usr/bin/env node
// Records a Claude Code session the way Ersa runs one, as a protocol fixture
// pair: the live stream and the CLI's own session record for the same run.
//
//   node scripts/record-claude-session.mjs <name> [--keep] [--cwd <dir>] [--resume <session id>] "<message 1>" ["<message 2>" ...]
//
// It auto-allows every tool call, and the CLI runs with the user's own
// settings loaded (`--setting-sources user,project,local`, as Ersa runs it), so
// whatever the model chooses to run, runs with the user's hooks and rules.
// Only ever run it in a scratch directory.
//
// From Git Bash, set MSYS_NO_PATHCONV=1: otherwise a message such as
// "/rename x" reaches the CLI as "C:/Program Files/Git/rename x".
//
// Starts the CLI with Ersa's command line and environment (PROTO-CLAUDE-CODE-STREAM,
// Invocation), sends `initialize` and waits for its answer, sends each message
// and waits for its `result`, allows every permission prompt, then closes stdin
// and waits for the exit. Then it copies the session record the CLI kept,
// `~/.claude/projects/<encoded cwd>/<session id>.jsonl`, as it stood after the run.
//
// Without --cwd it works in a new scratch git repository under the temp
// directory, never a real project. Afterwards it deletes that repository and
// the session record and registry file the run left in the real ~/.claude,
// unless --keep is given: then it prints the repository's path, so a later run
// can resume there with --cwd and --resume (and that run cleans up). Never
// point --cwd at a real project. Writes, under
// docs/briefs/protocol/fixtures/claude-code/<version>/:
//   <NN>-<name>.jsonl          the CLI's stdout
//   <NN>-<name>.meta.json      command line, environment changes, stdin lines, timings
//   record/<NN>-<name>.jsonl   the session record after the run
//
// Everything passes through redact() (scripts/redact.mjs) before it is written,
// and the OS user name is replaced wherever it appears, since streamed tool
// input can split a path mid-name. The record keeps verbatim the entries the
// adapter reads (READ_ENTRIES); every other entry is cut to its kind and its
// chain links (`uuid`, `parentUuid`, `logicalParentUuid`, `isSidechain`) and
// marked `"ersa_elided": true`, because those bodies are internal to the CLI
// and carry its system prompt and the recording machine's configuration.
// Set ERSA_CLAUDE to the claude executable if it is not on PATH.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir, userInfo } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
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

// Every line of the record that is not an entry the adapter reads becomes a
// marker; nothing else passes through, not even a line that does not parse.
function elide(recordText) {
  return recordText
    .split("\n")
    .map((line) => {
      if (!line.trim()) return line;
      let entry;
      try {
        entry = JSON.parse(line);
      } catch {
        return JSON.stringify({ type: "unparsable", ersa_elided: true });
      }
      if (entry === null || typeof entry !== "object" || Array.isArray(entry)) {
        return JSON.stringify({ type: "not-an-object", ersa_elided: true });
      }
      if (READ_ENTRIES.has(entry.type)) return line;
      const kept = { type: entry.type };
      if (entry.subtype !== undefined) kept.subtype = entry.subtype;
      // The links that make the conversation's chain, which runs through
      // these entries too.
      for (const key of ["uuid", "parentUuid", "logicalParentUuid", "isSidechain"]) {
        if (entry[key] !== undefined) kept[key] = entry[key];
      }
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
const keepIndex = argv.indexOf("--keep");
const keep = keepIndex !== -1;
if (keep) argv.splice(keepIndex, 1);
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
// Only a directory this script makes may be deleted by it.
const isScratch = basename(cwd).startsWith("ersa-rec-") && resolve(dirname(cwd)) === resolve(tmpdir());

// The CLI's directory name for a working directory
// (PROTO-CLAUDE-CODE-STREAM, Session record; long names verified on 2.1.316).
function encodeCwd(path) {
  const encoded = path.replace(/[^a-zA-Z0-9]/g, "-");
  if (encoded.length <= 200) return encoded;
  let h = 0;
  for (let i = 0; i < path.length; i++) h = ((h << 5) - h + path.charCodeAt(i)) | 0;
  return `${encoded.slice(0, 200)}-${Math.abs(h).toString(36)}`;
}

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
let exit = await Promise.race([exited, new Promise((r) => setTimeout(() => r(null), 30_000))]);
if (exit === null) {
  // Kill the whole tree (the CLI may be a shim with children), then wait for
  // the exit, so nothing is still writing the record when it is read.
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"]);
  } else {
    child.kill("SIGKILL");
  }
  await exited;
  exit = { code: null, signal: "timeout" };
}
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
let recordPath = null;
if (init) {
  recordPath = join(homedir(), ".claude", "projects", encodeCwd(init.cwd), `${init.session_id}.jsonl`);
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
// Leave nothing behind in the real ~/.claude or the temp directory, unless a
// resume run follows.
if (!keep) {
  if (recordPath) {
    rmSync(recordPath, { force: true });
    // The CLI keeps per-session folders (tool results, subagents) beside it.
    rmSync(recordPath.replace(/\.jsonl$/, ""), { recursive: true, force: true });
    // The CLI also makes empty folders there (`memory/`); the directory goes
    // only when no file is left in it, someone else's session included.
    const projectDir = dirname(recordPath);
    const holdsFiles = (d) =>
      readdirSync(d, { withFileTypes: true }).some((e) => !e.isDirectory() || holdsFiles(join(d, e.name)));
    if (existsSync(projectDir) && !holdsFiles(projectDir)) rmSync(projectDir, { recursive: true });
  }
  if (isScratch) rmSync(cwd, { recursive: true, force: true });
}
// A killed CLI cannot remove its registry file.
rmSync(join(homedir(), ".claude", "sessions", `${child.pid}.json`), { force: true });

console.log(`recorded ${stem} (${version}) in ${cwd}${keep ? " (kept)" : " (cleaned up)"}`);
console.log(`  session ${init?.session_id ?? "?"}: exit ${exit.code ?? exit.signal}, ${stdoutLines.length} lines out, ${sent.length} in, ${elapsed_ms} ms, record ${recordLines === null ? "NOT FOUND" : "copied"}`);
for (const t of turns) console.log(`  turn ${t.turn}: ${t.got_result ? t.subtype : "NO RESULT"}${t.is_error ? " (error)" : ""}`);
if (stderr) console.log("stderr:", stderr.trim().slice(0, 400));
