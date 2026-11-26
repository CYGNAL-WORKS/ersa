#!/usr/bin/env node
// Records Claude Code's sign-in, as a host with no terminal would start it,
// for the planning pass that writes PROTO-CLAUDE-CODE-STREAM's Signing in
// (ADR 0040, #633). A person signs in for real while it runs.
//
//   node scripts/record-claude-sign-in.mjs [--ending all|stopped|abandoned|finished]
//                                          [--route auto|control|process]
//                                          [--abandon-after <seconds>] [--wait <seconds>]
//                                          [--answer-within <seconds>] [--out <dir>]
//
// --answer-within (60 by default) is how long a CLI has to answer each request,
// and `auth login` to print its address, timed from that request or start.
//
// Each ending is one run, against a new scratch configuration folder
// (CLAUDE_CONFIG_DIR), so the machine's own sign-in is left as it is. The
// default, `all`, runs the three in turn: `stopped` (the host stops the
// sign-in as soon as it has an address; nobody signs in), `abandoned` (the
// person closes the page; the host stops the sign-in after --abandon-after
// seconds, 120 by default) and `finished` (the person signs in; the recorder
// waits up to --wait seconds, 600 by default).
//
// A run, in order:
//   1. starts a CLI signed out, `running`, with Ersa's command line, and sends it `initialize`;
//   2. starts the sign-in. The control route starts a second CLI the same way and
//      sends `claude_authenticate` with `loginWithClaudeAi`, then
//      `claude_oauth_wait_for_completion`. If the CLI refuses (an error answer, no
//      answer, an exit, or no `automaticUrl` to open) and --route is `auto`, the
//      process route follows: `claude auth login --claudeai` with a stdin nothing is
//      ever written to, since Ersa relays no code (ADR 0040). `claude_oauth_callback`
//      is never sent;
//   3. asks the person whether a sign-in page opened, and opens the address in
//      their browser if not (never for `stopped`);
//   4. waits for the ending, or stops the sign-in as the host would: the control
//      route withdraws the pending request (`control_cancel_request`), closes stdin,
//      then kills the process; the process route kills it;
//   5. sends `running` one message and records its answer;
//   6. probes a new CLI (`initialize`, `get_settings`);
//   7. after a finished sign-in, runs `claude auth logout` against the scratch folder,
//      so the credential is withdrawn before the folder is deleted.
//
// Nothing captured reaches disk until all of it has passed the check below.
// Every line is redacted (scripts/redact.mjs; the account's own email,
// organization and name are learned from what the CLI reports and replaced
// wherever they appear). Then, if any line still carries something shaped like
// a token, a key or an authorization code, or an account detail, the run fails
// and writes nothing: it names the file, line and kind, never the line. The
// recorder never prints a captured line or address.
//
// Writes, under docs/briefs/protocol/fixtures/claude-code/<version>/sign-in/
// (a folder of its own: the adapter's tests read every recording beside it),
// a `.jsonl` (stdout, one line per line; a text line is written as a JSON
// string) and a `.meta.json` per process: `<NN>-sign-in-<ending>`, the sign-in;
// `-running`, `-probe`, after a finished sign-in `-sign-out`, and a refused
// control route's process `-refused`.
//
// The scratch folder is deleted when the run ends, however it ends: the CLIs are
// killed first, so none holds its files. A run killed outright (Task Manager)
// leaves it; the next run of this recorder deletes it first.
//
// Set ERSA_CLAUDE to the claude executable if it is not on PATH (a `.mjs` file
// is run with node, as the tests' stand-in is). ERSA_SIGN_IN_OPENER names a node
// script to open the address with in place of the system's browser.

import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

import { ENV_REMOVED as ERSA_ENV_REMOVED, ENV_SET, ERSA_ARGS } from "./claude-invocation.mjs";
import { PLACEHOLDER_EMAIL, PLACEHOLDER_ORG, PLACEHOLDER_PLAN, accountDetails, escapeRegExp, nextFixtureNumber, redact, scrubUserName, secretsIn } from "./redact.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const CLAUDE = process.env.ERSA_CLAUDE || "claude";
// A `.mjs` stand-in is run with this node; the real CLI is run as itself.
const [EXE, EXE_ARGS] = CLAUDE.endsWith(".mjs") ? [process.execPath, [CLAUDE]] : [CLAUDE, []];

// Ersa's environment changes, and beside them every variable that would sign
// the CLI in without a sign-in: a key or a token.
const ENV_REMOVED = [...ERSA_ENV_REMOVED, "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"];
const MESSAGE = "Reply with the single word: ok";
const SCRATCH_PREFIX = "ersa-rec-sign-in-";

// ---- Arguments

const argv = process.argv.slice(2);
const option = (name, fallback) => {
  const i = argv.indexOf(name);
  return i === -1 ? fallback : argv[i + 1];
};
const endingArg = option("--ending", "all");
const route = option("--route", "auto");
const abandonAfter = Number(option("--abandon-after", "120")) * 1000;
const waitFor = Number(option("--wait", "600")) * 1000;
// How long a CLI has to answer a request or print its sign-in address.
const answerWithin = Number(option("--answer-within", "60")) * 1000;
const ENDINGS = endingArg === "all" ? ["stopped", "abandoned", "finished"] : [endingArg];
if (!ENDINGS.every((e) => ["stopped", "abandoned", "finished"].includes(e)) || !["auto", "control", "process"].includes(route) || !(abandonAfter >= 0) || !(waitFor > 0) || !(answerWithin > 0)) {
  console.error("usage: node scripts/record-claude-sign-in.mjs [--ending all|stopped|abandoned|finished] [--route auto|control|process] [--abandon-after <s>] [--wait <s>] [--answer-within <s>] [--out <dir>]");
  process.exit(1);
}

const version = spawnSync(EXE, [...EXE_ARGS, "--version"], { encoding: "utf8" }).stdout?.trim().split(/\s+/)[0];
if (!version) {
  console.error(`could not run ${CLAUDE} --version`);
  process.exit(1);
}
const OUT = option("--out", join(ROOT, "docs", "briefs", "protocol", "fixtures", "claude-code", version, "sign-in"));

// ---- Cleanup, on every ending a process can see

const children = new Set();
const scratchDirs = new Set();
function killTree(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === "win32") spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  else {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      // Already gone.
    }
  }
}
function removeScratch(dir) {
  try {
    rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
    scratchDirs.delete(dir);
  } catch {
    console.error(`could not delete the scratch folder ${dir}; delete it yourself: it may hold a credential`);
  }
}
// The CLIs first, so none still holds a file in the folder being deleted.
process.on("exit", () => {
  for (const child of children) killTree(child);
  for (const dir of scratchDirs) removeScratch(dir);
});
// Ctrl+C and a closed console end the run through `exit`. (Windows never
// delivers SIGTERM; a run killed outright is swept by the next one.)
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP", "SIGBREAK"]) process.on(signal, () => process.exit(130));

// Scratch folders an earlier run left when it was killed outright: its
// `recorder.pid` names a process that is no longer running.
function sweep() {
  for (const name of readdirSync(tmpdir()).filter((n) => n.startsWith(SCRATCH_PREFIX))) {
    const dir = join(tmpdir(), name);
    let alive = false;
    try {
      process.kill(Number(readFileSync(join(dir, "recorder.pid"), "utf8")), 0);
      alive = true;
    } catch (e) {
      alive = e.code === "EPERM";
    }
    if (!alive) removeScratch(dir);
  }
}

function makeScratch() {
  const dir = mkdtempSync(join(tmpdir(), SCRATCH_PREFIX));
  scratchDirs.add(dir);
  writeFileSync(join(dir, "recorder.pid"), String(process.pid));
  mkdirSync(join(dir, "config"));
  mkdirSync(join(dir, "cwd"));
  return dir;
}

// ---- The person at the terminal

const terminal = createInterface({ input: process.stdin });
const answers = [];
let answerWaiting = null;
let terminalClosed = false;
terminal.on("line", (line) => (answerWaiting ? (answerWaiting(line), (answerWaiting = null)) : answers.push(line)));
terminal.on("close", () => {
  terminalClosed = true;
  answerWaiting?.(null);
});
// Ctrl+C at a question reaches the reader, not the process.
terminal.on("SIGINT", () => process.exit(130));
function ask(question) {
  console.log(question);
  if (answers.length) return Promise.resolve(answers.shift());
  if (terminalClosed) return Promise.resolve(null);
  return new Promise((resolve) => (answerWaiting = resolve));
}

function openInBrowser(address) {
  const [cmd, args] = process.env.ERSA_SIGN_IN_OPENER
    ? [process.execPath, [process.env.ERSA_SIGN_IN_OPENER, address]]
    : process.platform === "win32"
      ? ["rundll32", ["url.dll,FileProtocolHandler", address]]
      : [process.platform === "darwin" ? "open" : "xdg-open", [address]];
  // No shell: an address's `&` must reach the browser as it is.
  spawn(cmd, args, { stdio: "ignore", detached: true }).unref();
}

// ---- A CLI process, every line each way timed

const delay = (ms) => new Promise((r) => setTimeout(r, ms));
const within = (promise, ms) => Promise.race([promise, delay(ms).then(() => null)]);

function startCli(args, env, cwd, started) {
  const child = spawn(EXE, [...EXE_ARGS, ...args], { cwd, env, detached: process.platform !== "win32", stdio: ["pipe", "pipe", "pipe"] });
  children.add(child);
  const proc = { args, child, stdout: [], stderr: [], sent: [], waiters: [] };
  proc.exited = new Promise((resolve) => {
    child.on("error", () => resolve({ code: null, signal: "spawn-error" }));
    child.on("exit", (code, signal) => resolve({ code, signal }));
  });
  child.stdin.on("error", () => {});
  const reader = (stream, into, onLine) => {
    let buffer = "";
    stream.on("data", (d) => {
      buffer += d.toString();
      let i;
      while ((i = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, i).replace(/\r$/, "");
        buffer = buffer.slice(i + 1);
        if (!line) continue;
        into.push({ t: Date.now() - started, line });
        onLine?.(line);
      }
    });
  };
  reader(child.stdout, proc.stdout, (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      msg = { text: line };
    }
    for (const w of [...proc.waiters]) {
      if (w.want(msg)) {
        proc.waiters.splice(proc.waiters.indexOf(w), 1);
        w.resolve(msg);
      }
    }
  });
  reader(child.stderr, proc.stderr);
  proc.send = (obj) => {
    const line = JSON.stringify(obj);
    proc.sent.push({ t: Date.now() - started, line });
    child.stdin.write(line + "\n");
  };
  // The next stdout line `want` accepts, or null after `ms`.
  proc.next = (want, ms) => {
    let waiter;
    const seen = new Promise((resolve) => proc.waiters.push((waiter = { want, resolve })));
    return within(seen, ms).then((msg) => {
      proc.waiters = proc.waiters.filter((w) => w !== waiter);
      return msg;
    });
  };
  let requests = 0;
  // A control request, and its answer (or null after `ms` or an exit).
  proc.control = (request, ms) => {
    const id = `ersa-${request.subtype}-${++requests}`;
    const answer = proc.next((m) => m.type === "control_response" && m.response?.request_id === id, ms);
    proc.send({ type: "control_request", request_id: id, request });
    return { id, answer: Promise.race([answer, proc.exited.then(() => null)]) };
  };
  // Closes stdin and waits for the exit, then kills what is left.
  proc.end = async (ms = 10_000) => {
    child.stdin.end();
    const exit = await within(proc.exited, ms);
    if (exit) return { ...exit, killed: false };
    killTree(child);
    return { ...(await proc.exited), killed: true };
  };
  return proc;
}

// ---- One ending

async function record(ending, number) {
  const started = Date.now();
  const scratch = makeScratch();
  const config = join(scratch, "config");
  const cwd = join(scratch, "cwd");
  const env = { ...process.env };
  for (const key of ENV_REMOVED) delete env[key];
  Object.assign(env, ENV_SET, { CLAUDE_CONFIG_DIR: config });
  const stream = () => [...ERSA_ARGS, "--session-id", randomUUID()];
  // The probe loads the user's settings alone, as the adapter's does (PROTO-CLAUDE-CODE-STREAM, Invocation).
  const probeArgs = () => stream().map((a) => (a === "user,project,local" ? "user" : a));
  const at = () => Date.now() - started;

  // 1. A session already running, signed out, before the sign-in.
  const running = startCli(stream(), env, cwd, started);
  const runningAnswerToInit = await running.control({ subtype: "initialize", hooks: {} }, answerWithin).answer;
  const runningInit = !!runningAnswerToInit;
  // The recording is of a sign-in only if the CLI starts signed out: the shape R83 reads.
  const account = runningAnswerToInit?.response?.response?.account;
  const startedSignedOut = account?.tokenSource === "none" && account?.apiProvider === "firstParty" && account?.apiKeySource === undefined;
  if (!startedSignedOut) console.error(`${ending}: WARNING: the CLI did not start signed out in the scratch folder, so this is not a recording of a sign-in`);

  // 2. The sign-in.
  const tried = [];
  const signIn = { tried, steps: [] };
  const step = (what) => signIn.steps.push({ t: at(), step: what });
  if (route !== "process") {
    const proc = startCli(stream(), env, cwd, started);
    const initialized = !!(await proc.control({ subtype: "initialize", hooks: {} }, answerWithin).answer);
    const auth = initialized ? await proc.control({ subtype: "claude_authenticate", loginWithClaudeAi: true }, answerWithin).answer : null;
    const address = auth?.response?.subtype === "success" ? auth.response.response?.automaticUrl : undefined;
    const refusal = !initialized ? "no answer to initialize" : !auth ? `no answer to claude_authenticate in ${answerWithin / 1000} s, or the CLI exited` : auth.response?.subtype !== "success" ? "claude_authenticate answered an error" : typeof address !== "string" ? "claude_authenticate answered no automaticUrl" : null;
    tried.push({ route: "control", refused: refusal });
    if (refusal) {
      step(`control route refused: ${refusal}`);
      const exit = await proc.end(5_000);
      signIn.refused = { proc, exit };
    } else {
      Object.assign(signIn, { route: "control", proc, address });
      step("claude_authenticate answered an address; claude_oauth_wait_for_completion sent");
      const wait = proc.control({ subtype: "claude_oauth_wait_for_completion" }, 24 * 3600_000);
      signIn.waitId = wait.id;
      signIn.done = wait.answer.then((m) => (m ? { how: m.response?.subtype === "success" ? "finished" : "failed", answer: m.response?.subtype } : { how: "exited" }));
    }
  }
  if (!signIn.route && route !== "control") {
    const proc = startCli(["auth", "login", "--claudeai"], env, cwd, started);
    // The address, from whichever stream prints it first.
    const fromOutput = () => [...proc.stdout, ...proc.stderr].map((l) => /https:\/\/\S+/.exec(l.line)?.[0]).find(Boolean);
    // Its own clock: the control route may have used up its time already.
    const since = Date.now();
    let address = fromOutput();
    while (!address && (await within(proc.exited, 100)) === null && Date.now() - since < answerWithin) address = fromOutput();
    tried.push({ route: "process", refused: address ? null : `printed no address in ${answerWithin / 1000} s, or exited first` });
    Object.assign(signIn, { route: "process", proc, address });
    signIn.done = proc.exited.then((e) => ({ how: e.code === 0 ? "finished" : "failed", answer: `exit ${e.code ?? e.signal}` }));
    step(address ? "auth login printed an address" : "auth login printed no address");
  }

  // 3. The page, and 4. the ending.
  let outcome = { how: "no route gave an address" };
  if (signIn.address) {
    if (ending !== "stopped") {
      const reply = await ask("Has a sign-in page opened in your browser? Type y or n, then press Enter.");
      signIn.person_said_page_opened = reply === null ? null : /^y/i.test(reply.trim());
      signIn.recorder_opened_address = signIn.person_said_page_opened === false;
      if (signIn.recorder_opened_address) {
        openInBrowser(signIn.address);
        step("the recorder opened the address in the browser");
      }
      console.log(ending === "finished" ? "Sign in on that page with your account. The recorder waits for you." : `Close that page without signing in. The recorder stops the sign-in in ${abandonAfter / 1000} seconds.`);
    }
    const limit = ending === "stopped" ? 0 : ending === "abandoned" ? abandonAfter : waitFor;
    const ended = await within(signIn.done, limit);
    if (ended) {
      outcome = ended;
      step(`the sign-in ended by itself: ${ended.how}`);
    } else {
      outcome = { how: `stopped by the host after ${limit / 1000} s` };
      await stopSignIn(signIn, step);
    }
  }
  // Whatever its ending, the sign-in's process is closed (and killed if it must be).
  if (signIn.proc) {
    signIn.exit = await signIn.proc.end(10_000);
    step(`the sign-in's process ${signIn.exit.killed ? "was killed" : "exited"}: ${signIn.exit.code ?? signIn.exit.signal}`);
  }

  // 5. The session that was running all along, asked once.
  let runningAnswer = null;
  if (runningInit) {
    running.send({ type: "user", message: { role: "user", content: [{ type: "text", text: MESSAGE }] } });
    runningAnswer = await running.next((m) => m.type === "result", 120_000);
  }
  const runningExit = await running.end();

  // 6. A fresh probe.
  const probe = startCli(probeArgs(), env, cwd, started);
  const probeInit = await probe.control({ subtype: "initialize", hooks: {} }, answerWithin).answer;
  if (probeInit) await probe.control({ subtype: "get_settings" }, answerWithin).answer;
  const probeExit = await probe.end();

  // 7. Withdraw the credential before the folder goes.
  let signOut = null;
  if (outcome.how === "finished") {
    signOut = startCli(["auth", "logout"], env, cwd, started);
    signOut.child.stdin.end();
    signOut.exit = (await within(signOut.exited, 60_000)) ?? (killTree(signOut.child), { ...(await signOut.exited), killed: true });
  }

  for (const child of children) killTree(child);
  removeScratch(scratch);

  // The recordings, then the check, then the write.
  const base = {
    recorded_at: new Date(started).toISOString(),
    claude_code_version: version,
    recorder: "scripts/record-claude-sign-in.mjs",
    ending,
    executable: CLAUDE,
    env_set: { ...ENV_SET, CLAUDE_CONFIG_DIR: "<a new scratch folder, deleted afterward>" },
    env_removed: ENV_REMOVED,
  };
  const meta = (proc, exit, extra) => ({
    ...base,
    ...extra,
    args: proc.args,
    exit_code: exit?.code ?? null,
    signal: exit?.signal ?? null,
    killed: exit?.killed ?? false,
    stdout_lines: proc.stdout.length,
    stdout_timing_ms: proc.stdout.map((l) => l.t),
    stderr: proc.stderr,
    sent: proc.sent,
  });
  const recordings = [];
  const add = (suffix, proc, exit, extra) => recordings.push({ suffix, proc, meta: meta(proc, exit, extra) });
  if (signIn.refused) add(`sign-in-${ending}-refused`, signIn.refused.proc, signIn.refused.exit, { role: "a sign-in by the control route, refused", refused: tried[0].refused });
  if (signIn.proc) {
    add(`sign-in-${ending}`, signIn.proc, signIn.exit, {
      role: "the sign-in",
      route: signIn.route,
      tried,
      outcome: outcome.how,
      final_answer: outcome.answer ?? null,
      address_field: signIn.route === "control" ? "automaticUrl" : "the first https address the process printed",
      person_said_page_opened: signIn.person_said_page_opened ?? null,
      recorder_opened_address: signIn.recorder_opened_address ?? false,
      steps: signIn.steps,
    });
  }
  add(`sign-in-${ending}-running`, running, runningExit, { role: "a session started signed out before the sign-in, then sent one message", message: MESSAGE, started_signed_out: startedSignedOut, got_result: !!runningAnswer });
  add(`sign-in-${ending}-probe`, probe, probeExit, { role: "a probe started after the sign-in ended" });
  if (signOut) add(`sign-in-${ending}-sign-out`, signOut, signOut.exit, { role: "claude auth logout against the scratch folder" });

  const learned = learnedValues(recordings.flatMap((r) => r.proc.stdout.map((l) => l.line)));
  const files = [];
  let n = Number(number);
  for (const r of recordings) {
    const stem = `${String(n++).padStart(2, "0")}-${r.suffix}`;
    const jsonl = r.proc.stdout.map((l) => (isJson(l.line) ? l.line : JSON.stringify(l.line))).join("\n") + "\n";
    files.push({ name: `${stem}.jsonl`, text: clean(jsonl, learned) });
    files.push({ name: `${stem}.meta.json`, text: clean(JSON.stringify(r.meta, null, 2) + "\n", learned) });
  }
  const findings = files.flatMap((f) => [
    ...secretsIn(f.text).map(({ line, kind }) => `${f.name}:${line}: ${kind}`),
    ...accountDetails(f.text).map(({ line, field }) => `${f.name}:${line}: ${field} as recorded`),
  ]);

  console.log(`${ending}: ${signIn.route ? `${signIn.route} route` : "no route"}${tried.some((t) => t.refused) ? ` (refused: ${tried.filter((t) => t.refused).map((t) => t.route).join(", ")})` : ""}, ${outcome.how}`);
  if (findings.length) {
    console.error(`${ending}: NOT WRITTEN. These lines carry what looks like a token, a key, a code or an account detail:`);
    for (const f of findings) console.error(`  ${f}`);
    return { written: [], failed: true, next: n };
  }
  mkdirSync(OUT, { recursive: true });
  for (const f of files) writeFileSync(join(OUT, f.name), f.text);
  console.log(`${ending}: wrote ${files.map((f) => f.name).join(", ")}`);
  // A run with no address recorded no sign-in: written, as evidence of the refusals, and failed.
  if (!signIn.address) console.error(`${ending}: no route gave a sign-in address, so nobody could sign in`);
  return { written: files.map((f) => f.name), failed: !startedSignedOut || !signIn.address, next: n };
}

// The host stops the sign-in: the control route withdraws the pending request,
// then closes stdin; the process route has no channel, so it is killed. Each
// step is timed; the process is closed (and killed if it must be) after.
async function stopSignIn(signIn, step) {
  if (signIn.route !== "control") {
    step("killed the sign-in process");
    killTree(signIn.proc.child);
    return;
  }
  signIn.proc.send({ type: "control_cancel_request", request_id: signIn.waitId });
  step("sent control_cancel_request for claude_oauth_wait_for_completion");
  const answered = await within(signIn.done, 5_000);
  step(answered ? `the wait answered after the cancel: ${answered.how}` : "the wait did not answer within 5 s of the cancel");
}

const isJson = (line) => {
  try {
    JSON.parse(line);
    return true;
  } catch {
    return false;
  }
};

// The account's own words, from every `account` object the CLI reported:
// the email and its local part, the organization, any name. The plan's name
// has its own rule (redact.mjs) and is short, so it is not learned.
function learnedValues(lines) {
  const values = new Set();
  const visit = (node, inAccount) => {
    if (!node || typeof node !== "object") return;
    for (const [key, value] of Object.entries(node)) {
      if (inAccount && typeof value === "string" && !/^(tokenSource|apiProvider|apiKeySource|subscriptionType)$/.test(key)) {
        values.add(value);
        if (value.includes("@")) values.add(value.split("@")[0]);
        if (/name/i.test(key)) for (const word of value.split(/\s+/)) values.add(word);
      }
      visit(value, inAccount || /^(account|oauthAccount|organization|profile)$/.test(key));
    }
  };
  for (const line of lines) {
    try {
      visit(JSON.parse(line), false);
    } catch {
      // Not JSON: nothing to learn from.
    }
  }
  // Long enough to be the account's own (a short word would rewrite others),
  // and never a placeholder.
  const placeholders = new Set([PLACEHOLDER_EMAIL, PLACEHOLDER_ORG, PLACEHOLDER_PLAN, "user", "redacted"]);
  return [...values].filter((v) => v.length >= 4 && !placeholders.has(v));
}

// The redactor's rules first, which put each field's own placeholder in
// place; then the account's words wherever else they appear, as whole words;
// then the machine's user name.
function clean(text, learned) {
  text = redact(text);
  for (const value of learned) {
    text = text.replace(new RegExp(`(?<![A-Za-z0-9])${escapeRegExp(value)}(?![A-Za-z0-9])`, "gi"), "redacted");
  }
  return scrubUserName(text);
}

// The machine's own credential, before and after: the recording must not touch it.
function credentialState() {
  const file = join(homedir(), ".claude", ".credentials.json");
  if (!existsSync(file)) return "absent";
  const s = statSync(file);
  return `${s.size}:${s.mtimeMs}`;
}

// ---- Main

sweep();
const credentialBefore = credentialState();
console.log(`Recording Claude Code ${version}'s sign-in: ${ENDINGS.join(", then ")}. Each runs against a new scratch folder, deleted afterward.`);
const GUIDE = {
  stopped: "The recorder starts a sign-in and stops it at once. If a sign-in page opens in your browser, close it. Don't sign in.",
  abandoned: `A sign-in page will open in your browser. Close it without signing in. The recorder then waits ${abandonAfter / 1000} seconds and stops the sign-in.`,
  finished: "A sign-in page will open in your browser. Sign in there with your account. The recorder waits for you to finish.",
};
let failed = false;
mkdirSync(OUT, { recursive: true });
let number = nextFixtureNumber(OUT);
for (const [i, ending] of ENDINGS.entries()) {
  await ask(`\n${i + 1} of ${ENDINGS.length}, ${ending}. ${GUIDE[ending]} Press Enter to start.`);
  const result = await record(ending, number);
  failed ||= result.failed;
  number = String(result.next).padStart(2, "0");
}
if (credentialState() !== credentialBefore) {
  console.error("WARNING: this machine's own Claude Code credential changed during the run. Run `claude auth status` to see whether it is signed in.");
  failed = true;
}
terminal.close();
console.log(failed ? "Done, with failures: see above." : `Done. Check the files in ${OUT} before committing them.`);
process.exit(failed ? 1 : 0);
