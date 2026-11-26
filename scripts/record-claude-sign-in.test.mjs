// Tests for record-claude-sign-in.mjs, against a stand-in CLI that plays each
// ending (scripts/fixtures/fake-claude-sign-in.mjs). They show the recorder
// ends, cleans up and refuses to write what it must not; they show nothing of
// what the real CLI does, which only the owner's run records.
// Run: node --test scripts/record-claude-sign-in.test.mjs

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { accountDetails, secretsIn } from "./redact.mjs";

const HERE = join(fileURLToPath(import.meta.url), "..");
const RECORDER = join(HERE, "record-claude-sign-in.mjs");
const FAKE = join(HERE, "fixtures", "fake-claude-sign-in.mjs");
// The stand-in's own account and token, which no written file may carry.
const EMAIL_LOCAL = "quillon.doe";
const TOKEN_START = "sk-ant-oat01-";
// Each run may take this long at most; a recorder that hangs fails here.
const LIMIT_MS = 60_000;

// A temp directory of the test's own, which the recorder sees as the system's:
// its scratch folders land there, so a test sees whether any is left.
function setup() {
  const root = mkdtempSync(join(tmpdir(), "ersa-sign-in-test-"));
  const temp = join(root, "temp");
  mkdirSync(temp);
  const out = join(root, "out");
  const trigger = join(root, "person-signed-in");
  const opened = join(root, "opened.txt");
  // The stand-in browser: the person signs in as soon as the page opens.
  const opener = join(root, "opener.mjs");
  writeFileSync(opener, `import { writeFileSync } from "node:fs";\nwriteFileSync(${JSON.stringify(opened)}, process.argv[2]);\nwriteFileSync(${JSON.stringify(trigger)}, "");\n`);
  return { root, temp, out, trigger, opened, opener, done: () => rmSync(root, { recursive: true, force: true }) };
}

// Runs the recorder with `args`, typing `input` at its questions.
function run(t, args, { input = "", play = "", keepRunning = false } = {}) {
  const env = {
    ...process.env,
    ERSA_CLAUDE: FAKE,
    ERSA_SIGN_IN_OPENER: t.opener,
    FAKE_SIGN_IN_PLAY: play,
    FAKE_SIGN_IN_TRIGGER: t.trigger,
    // A key the recorder must keep from the CLI: given it, the stand-in starts
    // with a key in place of a sign-in, and the run fails as not signed out.
    ANTHROPIC_API_KEY: "placeholder-key-for-tests",
    TEMP: t.temp,
    TMP: t.temp,
    TMPDIR: t.temp,
  };
  const child = spawn(process.execPath, [RECORDER, "--out", t.out, "--abandon-after", "1", "--wait", "30", ...args], { env, stdio: ["pipe", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (d) => (stdout += d));
  child.stderr.on("data", (d) => (stderr += d));
  child.stdin.write(input);
  if (!keepRunning) child.stdin.end();
  const exited = new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error(`the recorder did not end within ${LIMIT_MS} ms`));
    }, LIMIT_MS);
    child.on("exit", (code) => {
      clearTimeout(timer);
      resolve({ code, stdout, stderr });
    });
  });
  return { child, exited, output: () => stdout };
}

const scratchLeft = (t) => readdirSync(t.temp).filter((n) => n.startsWith("ersa-rec-sign-in-"));
const written = (t) => (existsSync(t.out) ? readdirSync(t.out).sort() : []);
const read = (t, name) => readFileSync(join(t.out, name), "utf8");
const metaOf = (t, name) => JSON.parse(read(t, name));

// Every file written: no token, account detail or the account's own words.
function assertClean(t) {
  for (const name of written(t)) {
    const text = read(t, name);
    assert.deepEqual(secretsIn(text), [], name);
    assert.deepEqual(accountDetails(text), [], name);
    assert.ok(!text.toLowerCase().includes(EMAIL_LOCAL) && !text.includes(TOKEN_START) && !text.includes("mail.example"), `${name} carries the account`);
  }
}

test("a finished sign-in by the control route is recorded with every process, redacted, and the scratch folder is gone", async () => {
  const t = setup();
  try {
    // Enter to start; "n": no page opened, so the recorder opens it and the person signs in.
    const { code, stdout } = await run(t, ["--ending", "finished"], { input: "\nn\n" }).exited;
    assert.equal(code, 0, stdout);
    assert.deepEqual(written(t), [
      "00-sign-in-finished.jsonl", "00-sign-in-finished.meta.json",
      "01-sign-in-finished-running.jsonl", "01-sign-in-finished-running.meta.json",
      "02-sign-in-finished-probe.jsonl", "02-sign-in-finished-probe.meta.json",
      "03-sign-in-finished-sign-out.jsonl", "03-sign-in-finished-sign-out.meta.json",
    ]);
    const signIn = metaOf(t, "00-sign-in-finished.meta.json");
    assert.equal(signIn.route, "control");
    assert.deepEqual(signIn.tried, [{ route: "control", refused: null }]);
    assert.equal(signIn.outcome, "finished");
    assert.equal(signIn.person_said_page_opened, false);
    assert.equal(signIn.recorder_opened_address, true);
    assert.deepEqual(signIn.sent.map((s) => JSON.parse(s.line).request.subtype), ["initialize", "claude_authenticate", "claude_oauth_wait_for_completion"]);
    // The recorder opened the address that needs no pasted code.
    assert.match(readFileSync(t.opened, "utf8"), /redirect_uri=http%3A%2F%2Flocalhost/);
    // Its one-time values are replaced in what was written.
    assert.match(read(t, "00-sign-in-finished.jsonl"), /code_challenge=REDACTED&code_challenge_method=S256&state=REDACTED/);
    // The account, as the redactor's placeholders.
    assert.match(read(t, "00-sign-in-finished.jsonl"), /"account":\{"email":"user@example.com","organization":"Example Org","subscriptionType":"Example Plan"/);
    // The session that was running before the sign-in was answered as signed in.
    assert.equal(metaOf(t, "01-sign-in-finished-running.meta.json").got_result, true);
    assert.equal(metaOf(t, "01-sign-in-finished-running.meta.json").started_signed_out, true);
    // The probe loads the user's settings alone, as the adapter's does.
    const probeArgs = metaOf(t, "02-sign-in-finished-probe.meta.json").args;
    assert.equal(probeArgs[probeArgs.indexOf("--setting-sources") + 1], "user");
    assert.match(read(t, "01-sign-in-finished-running.jsonl"), /"result":"ok"/);
    assert.match(read(t, "02-sign-in-finished-probe.jsonl"), /"email":"user@example.com"/);
    assert.equal(metaOf(t, "03-sign-in-finished-sign-out.meta.json").exit_code, 0);
    assertClean(t);
    assert.deepEqual(scratchLeft(t), []);
    assert.ok(!stdout.includes("https://"), "the recorder never prints the address");
  } finally {
    t.done();
  }
});

test("a control route the CLI refuses is noted, and the sign-in is recorded by the process route", async () => {
  const t = setup();
  try {
    const { code } = await run(t, ["--ending", "finished"], { input: "\nn\n", play: "refuse-control" }).exited;
    assert.equal(code, 0);
    assert.ok(written(t).includes("00-sign-in-finished-refused.jsonl"));
    const signIn = metaOf(t, "01-sign-in-finished.meta.json");
    assert.equal(signIn.route, "process");
    assert.deepEqual(signIn.tried, [
      { route: "control", refused: "claude_authenticate answered an error" },
      { route: "process", refused: null },
    ]);
    assert.deepEqual(signIn.args, ["auth", "login", "--claudeai"]);
    assert.equal(signIn.outcome, "finished");
    assert.equal(signIn.exit_code, 0);
    // A text line is kept as a JSON string, its address redacted.
    const lines = read(t, "01-sign-in-finished.jsonl").trim().split("\n").map((l) => JSON.parse(l));
    assert.ok(lines.every((l) => typeof l === "string"));
    assert.ok(lines.some((l) => l.includes("state=REDACTED")));
    assertClean(t);
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});

test("a control route that never answers falls back to the process route, which has its own time to print an address", async () => {
  const t = setup();
  try {
    const { code } = await run(t, ["--ending", "finished", "--answer-within", "1"], { input: "\nn\n", play: "silent-control" }).exited;
    assert.equal(code, 0);
    const signIn = metaOf(t, "01-sign-in-finished.meta.json");
    assert.deepEqual(signIn.tried, [
      { route: "control", refused: "no answer to claude_authenticate in 1 s, or the CLI exited" },
      { route: "process", refused: null },
    ]);
    assert.equal(signIn.outcome, "finished");
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});

test("a run where no route gives an address fails, and says so", async () => {
  const t = setup();
  try {
    const { code, stdout, stderr } = await run(t, ["--ending", "finished"], { input: "\n", play: "refuse-control,no-address" }).exited;
    assert.equal(code, 1);
    assert.match(stderr, /finished: no route gave a sign-in address/);
    assert.doesNotMatch(stdout, /^Done\. Check/m);
    assert.equal(metaOf(t, "01-sign-in-finished.meta.json").outcome, "no route gave an address");
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});

test("an abandoned sign-in is stopped by the host after the wait, and the recorder ends", async () => {
  const t = setup();
  try {
    // "y": the page opened (nothing is opened, so the person never signs in).
    const { code } = await run(t, ["--ending", "abandoned"], { input: "\ny\n" }).exited;
    assert.equal(code, 0);
    const signIn = metaOf(t, "00-sign-in-abandoned.meta.json");
    assert.equal(signIn.outcome, "stopped by the host after 1 s");
    assert.equal(signIn.person_said_page_opened, true);
    assert.equal(signIn.recorder_opened_address, false);
    assert.deepEqual(JSON.parse(signIn.sent.at(-1).line), { type: "control_cancel_request", request_id: "ersa-claude_oauth_wait_for_completion-3" });
    assert.ok(signIn.steps.some((s) => s.step === "sent control_cancel_request for claude_oauth_wait_for_completion"));
    // Still signed out afterward, and nothing to sign out of.
    assert.match(read(t, "01-sign-in-abandoned-running.jsonl"), /Not logged in/);
    assert.match(read(t, "02-sign-in-abandoned-probe.jsonl"), /"tokenSource":"none"/);
    assert.ok(!written(t).some((n) => n.includes("sign-out")));
    assert.ok(!existsSync(t.opened));
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});

test("an abandoned sign-in by the process route is killed by the host", async () => {
  const t = setup();
  try {
    const { code } = await run(t, ["--ending", "abandoned", "--route", "process"], { input: "\ny\n" }).exited;
    assert.equal(code, 0);
    const signIn = metaOf(t, "00-sign-in-abandoned.meta.json");
    assert.deepEqual(signIn.tried, [{ route: "process", refused: null }]);
    assert.equal(signIn.outcome, "stopped by the host after 1 s");
    assert.ok(signIn.steps.some((s) => s.step === "killed the sign-in process"));
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});

test("all three endings run in turn, stopped first, without asking whether a page opened for the stopped one", async () => {
  const t = setup();
  try {
    // stopped: Enter; abandoned: Enter, y; finished: Enter, n.
    const { code } = await run(t, [], { input: "\n\ny\n\nn\n" }).exited;
    assert.equal(code, 0);
    const signIns = written(t).filter((n) => /^\d+-sign-in-(stopped|abandoned|finished)\.meta\.json$/.test(n));
    assert.deepEqual(signIns, ["00-sign-in-stopped.meta.json", "03-sign-in-abandoned.meta.json", "06-sign-in-finished.meta.json"]);
    const stopped = metaOf(t, "00-sign-in-stopped.meta.json");
    assert.equal(stopped.outcome, "stopped by the host after 0 s");
    assert.equal(stopped.person_said_page_opened, null);
    assert.equal(metaOf(t, "06-sign-in-finished.meta.json").outcome, "finished");
    assertClean(t);
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});

test("a line carrying a token fails the recording: nothing is written and the token is not printed", async () => {
  const t = setup();
  try {
    const { code, stdout, stderr } = await run(t, ["--ending", "finished"], { input: "\nn\n", play: "leak-token" }).exited;
    assert.equal(code, 1);
    assert.deepEqual(written(t), []);
    assert.match(stderr, /finished: NOT WRITTEN/);
    assert.match(stderr, /00-sign-in-finished\.jsonl:\d+: an Anthropic key or token/);
    assert.ok(!stdout.includes(TOKEN_START) && !stderr.includes(TOKEN_START));
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});

test("the account's own words are replaced wherever the CLI writes them", async () => {
  const t = setup();
  try {
    const { code } = await run(t, ["--ending", "finished"], { input: "\nn\n", play: "leak-name" }).exited;
    assert.equal(code, 0);
    assert.match(read(t, "00-sign-in-finished.meta.json"), /welcome back, redacted; cache in D:\\\\builds\\\\user\\\\cache/);
    const name = userInfo().username.toLowerCase();
    if (name.length >= 3) for (const f of written(t)) assert.ok(!read(t, f).toLowerCase().includes(name), `${f} carries the machine's user name`);
    assertClean(t);
  } finally {
    t.done();
  }
});

test("a run killed outright leaves its scratch folder, and the next run deletes it", async () => {
  const t = setup();
  try {
    // Waits at the question, so the run is killed part way through.
    const first = run(t, ["--ending", "abandoned"], { input: "\n", keepRunning: true });
    while (!first.output().includes("Has a sign-in page opened")) await new Promise((r) => first.child.stdout.once("data", r));
    assert.equal(scratchLeft(t).length, 1);
    first.child.kill("SIGKILL");
    await first.exited;
    const { code } = await run(t, ["--ending", "stopped"], { input: "\n" }).exited;
    assert.equal(code, 0);
    assert.deepEqual(scratchLeft(t), []);
  } finally {
    t.done();
  }
});
