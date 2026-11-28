#!/usr/bin/env node
// Records Claude Code's sign-in, as a host with no terminal would start it,
// for the planning pass that writes PROTO-CLAUDE-CODE-STREAM's Signing in
// (ADR 0040, #633). A person signs in for real while it runs.
//
//   node scripts/record-claude-sign-in.mjs [--ending all|<ending>[,<ending>...]]
//                                          [--route auto|control|process]
//                                          [--abandon-after <seconds>] [--wait <seconds>]
//                                          [--answer-within <seconds>] [--settle <seconds>]
//                                          [--cli-version <x.y.z> [--offline]]
//                                          [--out <dir>]
//
// --answer-within (60 by default) is how long a CLI has to answer each request,
// and `auth login` to print its address, timed from that request or start.
// --settle (5 by default) is how long the host watches for the CLI to say
// anything after it withdraws a wait or starts a second sign-in.
//
// Each ending is one run, against a new scratch configuration folder
// (CLAUDE_CONFIG_DIR), so the machine's own sign-in is left as it is.
// --ending names one or more, comma-separated, run in the order given; `all`,
// the default, runs the three a person takes part in, and `no-account` the six
// that need none. Three need a person:
//   stopped               the host stops the sign-in as soon as it has an address; nobody signs in
//   abandoned             the person closes the page; the host stops the sign-in after
//                         --abandon-after seconds, 120 by default
//   finished              the person signs in; the recorder waits up to --wait seconds, 600 by
//                         default. The session running all along sends one message before the
//                         sign-in, which fails as not signed in, and one after it; then it is
//                         stopped and resumed, as the adapter resumes a session, and sent one
//                         more. At most two messages reach the model: the first fails at once.
// The rest need no account and no person: each opens no page and asks nothing,
// and each starts the sign-in by the control route alone, since `auth login`
// may open a browser by itself. Each is stopped as `stopped` is unless it says:
//   closed-after-cancel   the host withdraws the wait and closes stdin at once
//   closed-while-waiting  the host closes stdin with the wait pending, withdrawing nothing
//   second-same-process   a second claude_authenticate on the sign-in's process, while the
//                         first is under way
//   second-other-process  a second CLI started the same way, in the same scratch folder, starts
//                         a sign-in of its own while the first is under way; both are stopped
//   console-account       claude_authenticate with loginWithClaudeAi: false
//   key-in-environment    the sign-in's process alone is given ANTHROPIC_API_KEY, a placeholder
//                         shaped like no key
// After every ending, each process's meta says whether the scratch folder
// holds a session record for it (`left_session_record`).
//
// --cli-version records with another version of the CLI: npm installs that
// version's package for this platform (@anthropic-ai/claude-code-<platform>-<arch>),
// with its scripts off, into a scratch folder deleted when the run ends, as
// fixture 2.1.316/55-probe-get-settings was fetched. It downloads the CLI, so
// it is run only with the owner's yes. With --offline, npm takes the package
// from its own cache and fails rather than reach the network. ERSA_NPM names a
// node script to run in place of npm (the tests' stand-in).
//
// A run, in order:
//   1. starts a CLI signed out, `running`, with Ersa's command line, and sends it `initialize`
//      (for `finished`, then one message, whose answer it waits for);
//   2. starts the sign-in. The control route starts a second CLI the same way and
//      sends `claude_authenticate` with `loginWithClaudeAi`, then
//      `claude_oauth_wait_for_completion`. If the CLI refuses (an error answer, no
//      answer, an exit, or no `automaticUrl` to open) and --route is `auto`, the
//      process route follows: `claude auth login --claudeai` with a stdin nothing is
//      ever written to, since Ersa relays no code (ADR 0040). `claude_oauth_callback`
//      is never sent;
//   3. for `abandoned` and `finished`, asks the person whether a sign-in page opened, and
//      opens the address in their browser if not;
//   4. for the second-sign-in endings, starts the second sign-in; then waits for the
//      ending, or stops the sign-in as the host would: the control route withdraws the
//      pending request (`control_cancel_request`) and closes stdin, as the ending says,
//      then kills the process if it must; the process route kills it;
//   5. sends `running` one message and records its answer; for `finished`, then stops it,
//      resumes it (`-resumed`: Ersa's command line with `--resume <its id>`) and sends one more;
//   6. probes a new CLI (`initialize`, `get_settings`);
//   7. after a finished sign-in, runs `claude auth logout` against the scratch folder,
//      so the credential is withdrawn before the folder is deleted;
//   8. notes, before the folder is deleted, which processes left a session record in it.
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
// `-running`, for `finished` `-resumed`, `-probe`, after a finished sign-in `-sign-out`, and a refused
// control route's process `-refused`.
//
// The scratch folder is deleted when the run ends, however it ends: the CLIs are
// killed first, so none holds its files. A run killed outright (Task Manager)
// leaves it; the next run of this recorder deletes it first.
//
// Set ERSA_CLAUDE to the claude executable if it is not on PATH (a `.mjs` file
// is run with node, as the tests' stand-in is); --cli-version takes its place.
// ERSA_SIGN_IN_OPENER names a node script to open the address with in place of
// the system's browser.

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

// Ersa's environment changes, and beside them every variable that would sign
// the CLI in without a sign-in: a key or a token.
const ENV_REMOVED = [...ERSA_ENV_REMOVED, "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"];
const MESSAGE = "Reply with the single word: ok";
const SCRATCH_PREFIX = "ersa-rec-sign-in-";
// The key `key-in-environment` gives the sign-in's process: shaped like no
// key, so no check mistakes it for one and the vendor's service would refuse it.
const PLACEHOLDER_KEY = "placeholder-not-a-key";

// Each ending: whether the person is asked about the page (`ask`), how long
// the sign-in may run before the host stops it (`limit`), how the host stops
// it (`stop`), and what else it does. `controlOnly` endings never take the
// process route.
const ENDING = {
  stopped: { stop: "cancel" },
  abandoned: { ask: true, limit: () => abandonAfter, stop: "cancel" },
  finished: { ask: true, limit: () => waitFor, stop: "cancel", messageBefore: true, resume: true },
  "closed-after-cancel": { controlOnly: true, stop: "cancel-then-close" },
  "closed-while-waiting": { controlOnly: true, stop: "close" },
  "second-same-process": { controlOnly: true, stop: "cancel", second: "same" },
  "second-other-process": { controlOnly: true, stop: "cancel", second: "other" },
  "console-account": { controlOnly: true, stop: "cancel", claudeAi: false },
  "key-in-environment": { controlOnly: true, stop: "cancel", keyInEnvironment: true },
};

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
// How long the host watches for the CLI to say anything after a cancel or a
// second sign-in, before going on.
const settle = Number(option("--settle", "5")) * 1000;
const cliVersion = option("--cli-version", null);
const offline = argv.includes("--offline");
// `all` is the three a person takes part in; `no-account` the rest.
const NO_ACCOUNT = Object.keys(ENDING).filter((name) => ENDING[name].controlOnly);
const ENDINGS = endingArg === "all" ? ["stopped", "abandoned", "finished"] : endingArg === "no-account" ? NO_ACCOUNT : endingArg.split(",");
// The version reaches a command line, so it is digits and dots and nothing else.
if (!ENDINGS.every((e) => Object.hasOwn(ENDING, e)) || !["auto", "control", "process"].includes(route) || !(abandonAfter >= 0) || !(waitFor > 0) || !(answerWithin > 0) || !(settle >= 0) || (cliVersion !== null && !/^\d+\.\d+\.\d+$/.test(cliVersion)) || (offline && cliVersion === null)) {
  console.error(`usage: node scripts/record-claude-sign-in.mjs [--ending all|<ending>[,<ending>...]] [--route auto|control|process] [--abandon-after <s>] [--wait <s>] [--answer-within <s>] [--settle <s>] [--cli-version <x.y.z> [--offline]] [--out <dir>]\nendings: ${Object.keys(ENDING).join(", ")}`);
  process.exit(1);
}

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

// ---- The CLI

// Another version's CLI, installed by npm into a scratch folder of its own:
// the package for this platform, with none of its scripts run. Returns the
// executable, or exits naming what failed.
function installCli(wanted) {
  const name = `@anthropic-ai/claude-code-${process.platform}-${process.arch}`;
  const spec = `${name}@${wanted}`;
  const prefix = join(makeScratch(), "cli");
  const npmArgs = ["install", "--prefix", prefix, "--no-save", "--no-package-lock", "--ignore-scripts", "--no-audit", "--no-fund", ...(offline ? ["--offline"] : []), spec];
  console.log(`Installing ${spec}${offline ? " from npm's cache" : ""} into a scratch folder, deleted when the run ends.`);
  const result = process.env.ERSA_NPM
    ? spawnSync(process.execPath, [process.env.ERSA_NPM, ...npmArgs], { encoding: "utf8" })
    : // npm is `npm.cmd` on Windows, which runs only through a shell. Every
      // argument is a fixed word, a checked version or a temp path, quoted.
      spawnSync(["npm", ...npmArgs].map((a) => (a === prefix ? `"${a}"` : a)).join(" "), { encoding: "utf8", shell: true });
  const dir = join(prefix, "node_modules", ...name.split("/"));
  if (result.status !== 0 || !existsSync(join(dir, "package.json"))) {
    console.error(`npm could not install ${spec} (exit ${result.status ?? result.error?.code}):\n${result.stderr ?? ""}`);
    process.exit(1);
  }
  // The package's own `bin` for `claude` where it names one, else the
  // executable at its root, where 2.1.316's package had it.
  const { bin } = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  const exe = [typeof bin === "string" ? bin : bin?.claude, process.platform === "win32" ? "claude.exe" : "claude"]
    .filter(Boolean)
    .map((b) => join(dir, b))
    .find(existsSync);
  if (!exe) {
    console.error(`${spec} holds no claude executable`);
    process.exit(1);
  }
  return { exe, described: `${spec}, installed by npm into a scratch folder, deleted afterward` };
}

const installed = cliVersion ? installCli(cliVersion) : null;
const CLAUDE = installed?.exe ?? (process.env.ERSA_CLAUDE || "claude");
// A script (the tests' `.mjs` stand-in, or a package's `.js` bin) is run with
// this node; the real CLI is run as itself.
const [EXE, EXE_ARGS] = /\.[cm]?js$/.test(CLAUDE) ? [process.execPath, [CLAUDE]] : [CLAUDE, []];

const version = spawnSync(EXE, [...EXE_ARGS, "--version"], { encoding: "utf8" }).stdout?.trim().split(/\s+/)[0];
if (!version) {
  console.error(`could not run ${installed?.described ?? CLAUDE} --version`);
  process.exit(1);
}
if (cliVersion && version !== cliVersion) {
  console.error(`the installed CLI says it is ${version}, not ${cliVersion}; nothing recorded`);
  process.exit(1);
}
const OUT = option("--out", join(ROOT, "docs", "briefs", "protocol", "fixtures", "claude-code", version, "sign-in"));

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
  const e = ENDING[ending];
  const started = Date.now();
  const scratch = makeScratch();
  const config = join(scratch, "config");
  const cwd = join(scratch, "cwd");
  const env = { ...process.env };
  for (const key of ENV_REMOVED) delete env[key];
  Object.assign(env, ENV_SET, { CLAUDE_CONFIG_DIR: config });
  // The sign-in's processes, which `key-in-environment` alone gives a key.
  const signInEnv = e.keyInEnvironment ? { ...env, ANTHROPIC_API_KEY: PLACEHOLDER_KEY } : env;
  const stream = () => [...ERSA_ARGS, "--session-id", randomUUID()];
  // The probe loads the user's settings alone, as the adapter's does (PROTO-CLAUDE-CODE-STREAM, Invocation).
  const probeArgs = () => stream().map((a) => (a === "user,project,local" ? "user" : a));
  const at = () => Date.now() - started;
  const endingRoute = e.controlOnly ? "control" : route;

  // 1. A session already running, signed out, before the sign-in.
  const running = startCli(stream(), env, cwd, started);
  const runningAnswerToInit = await running.control({ subtype: "initialize", hooks: {} }, answerWithin).answer;
  const runningInit = !!runningAnswerToInit;
  // The recording is of a sign-in only if the CLI starts signed out: the shape R83 reads.
  const account = runningAnswerToInit?.response?.response?.account;
  const startedSignedOut = account?.tokenSource === "none" && account?.apiProvider === "firstParty" && account?.apiKeySource === undefined;
  if (!startedSignedOut) console.error(`${ending}: WARNING: the CLI did not start signed out in the scratch folder, so this is not a recording of a sign-in`);
  // One message to a session, kept in its `messages` with whether it was
  // answered and whether as an error. A CLI that exits instead of answering
  // ends the wait at once.
  const sendMessage = async (proc, when) => {
    const answer = Promise.race([proc.next((m) => m.type === "result", 120_000), proc.exited.then(() => null)]);
    proc.send({ type: "user", message: { role: "user", content: [{ type: "text", text: MESSAGE }] } });
    const result = await answer;
    proc.messages.push({ when, got_result: !!result, is_error: result?.is_error ?? null });
    return result;
  };
  running.messages = [];
  // `finished`: one message before the sign-in, which must fail as not signed
  // in, or this is not the case the recording is for.
  let failedBefore = true;
  if (runningInit && e.messageBefore) {
    failedBefore = (await sendMessage(running, "before the sign-in"))?.is_error === true;
    if (!failedBefore) console.error(`${ending}: WARNING: the message sent before the sign-in did not fail as not signed in`);
  }

  // 2. The sign-in.
  const tried = [];
  const signIn = { tried, steps: [] };
  const step = (what) => signIn.steps.push({ t: at(), step: what });
  if (endingRoute !== "process") {
    const control = await startByControl(signInEnv, cwd, started, stream, e.claudeAi ?? true);
    tried.push({ route: "control", refused: control.refused });
    if (control.refused) {
      step(`control route refused: ${control.refused}`);
      signIn.refused = { proc: control.proc, exit: await control.proc.end(5_000) };
    } else {
      Object.assign(signIn, control, { route: "control" });
      step("claude_authenticate answered an address; claude_oauth_wait_for_completion sent");
    }
  }
  if (!signIn.route && endingRoute !== "control") {
    const proc = startCli(["auth", "login", "--claudeai"], signInEnv, cwd, started);
    // The address, from whichever stream prints it first.
    const fromOutput = () => [...proc.stdout, ...proc.stderr].map((l) => /https:\/\/\S+/.exec(l.line)?.[0]).find(Boolean);
    // Its own clock: the control route may have used up its time already.
    const since = Date.now();
    let address = fromOutput();
    while (!address && (await within(proc.exited, 100)) === null && Date.now() - since < answerWithin) address = fromOutput();
    tried.push({ route: "process", refused: address ? null : `printed no address in ${answerWithin / 1000} s, or exited first` });
    Object.assign(signIn, { route: "process", proc, address });
    signIn.done = proc.exited.then((x) => ({ how: x.code === 0 ? "finished" : "failed", answer: `exit ${x.code ?? x.signal}` }));
    step(address ? "auth login printed an address" : "auth login printed no address");
  }

  // 3. The page, and 4. the ending.
  let outcome = { how: "no route gave an address" };
  let second = null;
  if (signIn.address) {
    if (e.ask) {
      const reply = await ask("Has a sign-in page opened in your browser? Type y or n, then press Enter.");
      signIn.person_said_page_opened = reply === null ? null : /^y/i.test(reply.trim());
      signIn.recorder_opened_address = signIn.person_said_page_opened === false;
      if (signIn.recorder_opened_address) {
        openInBrowser(signIn.address);
        step("the recorder opened the address in the browser");
      }
      console.log(ending === "finished" ? "Sign in on that page with your account. The recorder waits for you." : `Close that page without signing in. The recorder stops the sign-in in ${abandonAfter / 1000} seconds.`);
    }
    // A second sign-in while this one is under way, on its process or another.
    if (e.second === "same") {
      const again = await signIn.proc.control({ subtype: "claude_authenticate", loginWithClaudeAi: true }, answerWithin).answer;
      step(again ? `the second claude_authenticate answered: ${again.response?.subtype}${again.response?.response?.automaticUrl ? ", with an address" : ""}` : `the second claude_authenticate was not answered in ${answerWithin / 1000} s, or the CLI exited`);
    } else if (e.second === "other") {
      second = await startByControl(signInEnv, cwd, started, stream, true);
      step(second.refused ? `the second process's sign-in was refused: ${second.refused}` : "the second process's claude_authenticate answered an address; its claude_oauth_wait_for_completion sent");
    }
    if (e.second) {
      const first = await within(signIn.done, settle);
      step(first ? `the first wait answered after the second sign-in: ${first.how}` : `the first wait did not answer within ${settle / 1000} s of the second sign-in`);
      if (second?.done) {
        const answered = await within(second.done, 0);
        step(answered ? `the second process's wait answered: ${answered.how}` : "the second process's wait had not answered");
      }
    }
    const limit = e.limit?.() ?? 0;
    const ended = await within(signIn.done, limit);
    if (ended) {
      outcome = ended;
      step(`the sign-in ended by itself: ${ended.how}`);
    } else {
      outcome = { how: `stopped by the host after ${limit / 1000} s` };
      await stopSignIn(signIn, e.stop, step);
    }
    if (second?.done) {
      const ended2 = await within(second.done, 0);
      second.outcome = ended2 ? ended2.how : "stopped by the host";
      if (!ended2) await stopSignIn(second, e.stop, (what) => step(`second process: ${what}`));
    }
  }
  // Whatever its ending, the sign-in's process is closed (and killed if it must be).
  if (signIn.proc) {
    signIn.exit = await signIn.proc.end(10_000);
    step(`the sign-in's process ${signIn.exit.killed ? "was killed" : "exited"}: ${signIn.exit.code ?? signIn.exit.signal}`);
  }
  if (second) {
    second.exit = await second.proc.end(10_000);
    step(`the second process ${second.exit.killed ? "was killed" : "exited"}: ${second.exit.code ?? second.exit.signal}`);
  }

  // 5. The session that was running all along, asked once more.
  let runningAnswer = null;
  if (runningInit) runningAnswer = await sendMessage(running, "after the sign-in ended");
  // `finished`: stopped as the adapter stops a session (stdin closed, then
  // killed after 3 s), so the next step resumes it from its record.
  const runningExit = await running.end(e.resume ? 3_000 : undefined);

  // 5a. `finished`: that session resumed as the adapter resumes one (Ersa's
  // command line with --resume and its id, in its folder), whether or not its
  // message above was answered, and sent one more message. A resume the CLI
  // refuses (no answer to `initialize`) is recorded as it happens.
  let resumed = null;
  if (e.resume && runningInit) {
    const id = running.args[running.args.indexOf("--session-id") + 1];
    resumed = startCli([...ERSA_ARGS, "--resume", id], env, cwd, started);
    resumed.messages = [];
    resumed.resumedId = id;
    resumed.answeredInit = !!(await resumed.control({ subtype: "initialize", hooks: {} }, answerWithin).answer);
    if (resumed.answeredInit) await sendMessage(resumed, "after the session was resumed");
    resumed.exit = await resumed.end(3_000);
  }

  // 6. A fresh probe.
  const probe = startCli(probeArgs(), env, cwd, started);
  const probeInit = await probe.control({ subtype: "initialize", hooks: {} }, answerWithin).answer;
  if (probeInit) await probe.control({ subtype: "get_settings" }, answerWithin).answer;
  const probeExit = await probe.end();

  // 7. Withdraw the credential before the folder goes.
  let signOut = null;
  if (outcome.how === "finished" || second?.outcome === "finished") {
    signOut = startCli(["auth", "logout"], env, cwd, started);
    signOut.child.stdin.end();
    signOut.exit = (await within(signOut.exited, 60_000)) ?? (killTree(signOut.child), { ...(await signOut.exited), killed: true });
  }

  for (const child of children) killTree(child);
  // What the folder holds of each process, now that none is running.
  const records = sessionRecords(join(config, "projects"));
  removeScratch(scratch);

  // The recordings, then the check, then the write.
  const base = {
    recorded_at: new Date(started).toISOString(),
    claude_code_version: version,
    recorder: "scripts/record-claude-sign-in.mjs",
    ending,
    executable: installed?.described ?? CLAUDE,
    env_set: { ...ENV_SET, CLAUDE_CONFIG_DIR: "<a new scratch folder, deleted afterward>" },
    env_removed: ENV_REMOVED,
  };
  // The session a process reported in its `system` `init` lines, the last one.
  const reportedSession = (proc) =>
    proc.stdout.map((l) => (isJson(l.line) ? JSON.parse(l.line) : null)).filter((m) => m?.type === "system" && m.subtype === "init").at(-1)?.session_id ?? null;
  const meta = (proc, exit, extra) => {
    // A resumed process's session is the one it reports, which may be new.
    const id = proc.resumedId ? reportedSession(proc) : proc.args[proc.args.indexOf("--session-id") + 1];
    return {
      ...base,
      ...(proc.env === signInEnv && e.keyInEnvironment ? { env_set_for_this_process: { ANTHROPIC_API_KEY: "<a placeholder shaped like no key>" } } : {}),
      ...extra,
      args: proc.args,
      exit_code: exit?.code ?? null,
      signal: exit?.signal ?? null,
      killed: exit?.killed ?? false,
      // A stream-json process names its session; `auth` commands have none.
      left_session_record: proc.args.includes("--session-id") || (proc.resumedId && id) ? records.has(`${id}.jsonl`) : null,
      stdout_lines: proc.stdout.length,
      stdout_timing_ms: proc.stdout.map((l) => l.t),
      stderr: proc.stderr,
      sent: proc.sent,
    };
  };
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
  if (second) add(`sign-in-${ending}-second`, second.proc, second.exit, { role: "a second process's sign-in, started while the first was under way", refused: second.refused, outcome: second.outcome ?? null });
  add(`sign-in-${ending}-running`, running, runningExit, {
    role: e.messageBefore ? "a session started signed out before the sign-in, sent one message before it and one after" : "a session started signed out before the sign-in, then sent one message",
    message: MESSAGE,
    started_signed_out: startedSignedOut,
    messages: running.messages,
    got_result: !!runningAnswer,
  });
  if (resumed) {
    const reported = reportedSession(resumed);
    add(`sign-in-${ending}-resumed`, resumed, resumed.exit, {
      role: "the session that ran all along, stopped after the sign-in and resumed, then sent one message",
      message: MESSAGE,
      answered_initialize: resumed.answeredInit,
      // Whether the resumed process carried on the session it was given, by the
      // id its `system` `init` reported; null when it reported none.
      resumed_session_id_matches: reported === null ? null : reported === resumed.resumedId,
      messages: resumed.messages,
    });
  }
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
  return { written: files.map((f) => f.name), failed: !startedSignedOut || !failedBefore || !signIn.address, next: n };
}

// A sign-in by the control route: a CLI started with Ersa's command line,
// `initialize`, `claude_authenticate`, then `claude_oauth_wait_for_completion`
// once there is an address. Returns the process and, when the CLI refused (an
// error answer, no answer, an exit, or no `automaticUrl` to open), why.
async function startByControl(env, cwd, started, stream, loginWithClaudeAi) {
  const proc = startCli(stream(), env, cwd, started);
  proc.env = env;
  const initialized = !!(await proc.control({ subtype: "initialize", hooks: {} }, answerWithin).answer);
  const auth = initialized ? await proc.control({ subtype: "claude_authenticate", loginWithClaudeAi }, answerWithin).answer : null;
  const address = auth?.response?.subtype === "success" ? auth.response.response?.automaticUrl : undefined;
  const refused = !initialized ? "no answer to initialize" : !auth ? `no answer to claude_authenticate in ${answerWithin / 1000} s, or the CLI exited` : auth.response?.subtype !== "success" ? "claude_authenticate answered an error" : typeof address !== "string" ? "claude_authenticate answered no automaticUrl" : null;
  if (refused) return { proc, refused };
  const wait = proc.control({ subtype: "claude_oauth_wait_for_completion" }, 24 * 3600_000);
  const done = wait.answer.then((m) => (m ? { how: m.response?.subtype === "success" ? "finished" : "failed", answer: m.response?.subtype } : { how: "exited" }));
  return { proc, refused: null, address, waitId: wait.id, done };
}

// The host stops the sign-in. By the control route, as `how` says: `cancel`
// withdraws the pending wait and gives the CLI --settle seconds to say anything;
// `cancel-then-close` withdraws it and closes stdin at once; `close` closes
// stdin with the wait still pending. The process route has no channel, so it
// is killed. Each step is timed; the process is closed (and killed if it must
// be) after.
async function stopSignIn(signIn, how, step) {
  if (signIn.route === "process") {
    step("killed the sign-in process");
    killTree(signIn.proc.child);
    return;
  }
  if (how !== "close") {
    signIn.proc.send({ type: "control_cancel_request", request_id: signIn.waitId });
    step("sent control_cancel_request for claude_oauth_wait_for_completion");
  }
  if (how === "cancel") {
    const answered = await within(signIn.done, settle);
    step(answered ? `the wait answered after the cancel: ${answered.how}` : `the wait did not answer within ${settle / 1000} s of the cancel`);
    return;
  }
  signIn.proc.child.stdin.end();
  step(how === "close" ? "closed stdin with the wait pending, withdrawing nothing" : "closed stdin straight after the cancel");
  const answered = await within(signIn.done, 0);
  if (answered) step(`the wait answered: ${answered.how}`);
}

// The names of the session record files anywhere under `projects/`, which a
// process's `<session id>.jsonl` would be one of whatever its folder is named.
function sessionRecords(projects) {
  if (!existsSync(projects)) return new Set();
  return new Set(readdirSync(projects, { recursive: true }).map((p) => String(p).split(/[\\/]/).at(-1)).filter((n) => n.endsWith(".jsonl")));
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
  finished: "A session sends one message first, which fails as not signed in. Then a sign-in page will open in your browser. Sign in there with your account. The recorder waits for you to finish. Then the session sends one more message, is stopped and resumed, and sends one more.",
};
// The endings that need no person: nothing opens and nothing is asked but Enter.
for (const name of NO_ACCOUNT) GUIDE[name] = `The recorder starts a sign-in (${name}) and stops it. No page opens. Don't sign in.`;
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
