// Tests for redact.mjs, and a guard over every committed recording and golden
// file. Run: node --test scripts/redact.test.mjs

import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  CHECKOUT,
  EMAIL_RE,
  HOME_RE,
  MUNGED_HOME_RE,
  PLACEHOLDER_EMAIL,
  PLACEHOLDER_HOME,
  PLACEHOLDER_MUNGED_REPO,
  PLACEHOLDER_ORG,
  PLACEHOLDER_PLAN,
  PLACEHOLDER_REPO,
  PLACEHOLDER_USAGE,
  PLACEHOLDER_UUID,
  REPO,
  accountDetails,
  expandMcpConfig,
  expandRepo,
  nextFixtureNumber,
  redact,
  repoRules,
  scrubUserName,
  secretsIn,
} from "./redact.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");

test("redacts the account email and organization", () => {
  const line = '{"account":{"email":"jo.doe@mail.com","organization":"jo.doe@mail.com\'s Organization"}}';
  assert.equal(redact(line), `{"account":{"email":"${PLACEHOLDER_EMAIL}","organization":"${PLACEHOLDER_ORG}"}}`);
});

test("redacts the organization id", () => {
  const line = '{"type":"credential_org","organizationUuid":"1b2c3d4e-0000-4000-8000-123456789abc"}';
  assert.equal(redact(line), `{"type":"credential_org","organizationUuid":"${PLACEHOLDER_UUID}"}`);
});

test("redacts the home directory in every spelling", () => {
  for (const [input, rest, expected] of [
    ["C:\\Users\\jo\\AppData", "\\AppData"],
    ["C:/Users/jo/AppData", "/AppData"],
    ["C:\\\\Users\\\\jo\\\\AppData", "\\\\AppData"],
    ["c:\\users\\jo", ""],
    ["/Users/jo/src", "/src"],
    ["/home/jo/src", "", "/home/user/src"],
    ["projects\\C--Users-jo-AppData", "", "projects\\C--Users-user-AppData"],
  ]) {
    assert.equal(redact(input), expected ?? PLACEHOLDER_HOME + rest, input);
  }
});

test("redacts the repository's path in every spelling, plain and munged, and leaves its neighbours", () => {
  assert.ok(REPO.includes("/"), REPO);
  const back = REPO.replace(/\//g, "\\");
  const escaped = REPO.replace(/\//g, "\\\\");
  const munged = REPO.replace(/[^A-Za-z0-9]/g, "-");
  for (const [input, expected] of [
    [`${REPO}/scripts/x.mjs`, `${PLACEHOLDER_REPO}/scripts/x.mjs`],
    [`cwd ${back}\\.claude\\worktrees\\w`, `cwd ${PLACEHOLDER_REPO}\\.claude\\worktrees\\w`],
    [`"${escaped}\\\\a"`, `"${PLACEHOLDER_REPO}\\\\a"`],
    [REPO.toLowerCase(), PLACEHOLDER_REPO],
    [`Temp\\claude\\${munged}\\00000000-0000-0000-0000-000000000000`, `Temp\\claude\\${PLACEHOLDER_MUNGED_REPO}\\00000000-0000-0000-0000-000000000000`],
    [`projects\\${munged}--claude-worktrees-w\\memory`, `projects\\${PLACEHOLDER_MUNGED_REPO}--claude-worktrees-w\\memory`],
    [`(${REPO}) ${REPO}. ${REPO}:12 ${REPO}.git`, `(${PLACEHOLDER_REPO}) ${PLACEHOLDER_REPO}. ${PLACEHOLDER_REPO}:12 ${PLACEHOLDER_REPO}.git`],
  ]) {
    assert.equal(redact(input), expected, input);
    assert.equal(redact(expected), expected, expected);
  }
  // Neighbours are other folders: the repository rule leaves them alone (the
  // home rule may still rewrite them on a runner whose checkout is under /home).
  const neighbours = redact(`${REPO}2/x ${REPO}-x ${munged}2`);
  assert.ok(!neighbours.includes(PLACEHOLDER_REPO) && !neighbours.includes(PLACEHOLDER_MUNGED_REPO), neighbours);
});

test("the repository rules hold for a Unix path, whose munged form starts with a dash", () => {
  const { plain, munged } = repoRules("/home/runner/work/ersa/ersa");
  assert.equal("cwd /home/runner/work/ersa/ersa/x (/home/runner/work/ersa/ersa).".replace(plain, "R"), "cwd R/x (R).");
  assert.equal("/home/runner/work/ersa/ersa2 \\home\\runner\\work\\ersa\\ersa".replace(plain, "R"), "/home/runner/work/ersa/ersa2 R");
  assert.equal("projects\\-home-runner-work-ersa-ersa--claude-w".replace(munged, "M"), "projects\\M--claude-w");
  assert.equal("x-home-runner-work-ersa-ersa".replace(munged, "M"), "x-home-runner-work-ersa-ersa");
});

test("expands the repository placeholder, to this checkout, for a file the CLI reads", () => {
  assert.ok(CHECKOUT === REPO || CHECKOUT.startsWith(REPO + "/"), `${CHECKOUT} under ${REPO}`);
  assert.equal(redact(`${CHECKOUT}/x`), `${PLACEHOLDER_REPO}${CHECKOUT.slice(REPO.length)}/x`);
  assert.equal(expandRepo(`{"args":["${PLACEHOLDER_REPO}/scripts/s.mjs"]}`), `{"args":["${CHECKOUT}/scripts/s.mjs"]}`);
  const dir = mkdtempSync(join(tmpdir(), "ersa-redact-"));
  try {
    const fixture = join(dir, "fixture.json");
    writeFileSync(fixture, `{"cwd":"${PLACEHOLDER_REPO}"}`);
    const args = ["--max-turns", "3", "--mcp-config", fixture, "prompt"];
    const cli = expandMcpConfig(args);
    assert.deepEqual(cli.toSpliced(3, 1), ["--max-turns", "3", "--mcp-config", "prompt"]);
    assert.notEqual(dirname(cli[3]), dir, "the copy lives in a scratch directory of its own");
    assert.deepEqual(args, ["--max-turns", "3", "--mcp-config", fixture, "prompt"], "the recorded arguments keep the file as given");
    assert.equal(readFileSync(cli[3], "utf8"), `{"cwd":"${CHECKOUT}"}`);
    rmSync(dirname(cli[3]), { recursive: true, force: true });
    const none = ["--max-turns", "3"];
    assert.equal(expandMcpConfig(none), none);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("leaves other paths and placeholders alone", () => {
  const text = `C:\\Windows\\System32 C:\\Users\\Public\\x ${PLACEHOLDER_HOME}/x ${PLACEHOLDER_EMAIL} ${PLACEHOLDER_REPO}/y`;
  assert.equal(redact(text), text);
});

test("redacts the plan's name", () => {
  const line = '{"account":{"organization":"Example Org","subscriptionType":"Claude Mega" }}';
  const out = redact(line);
  assert.equal(out, `{"account":{"organization":"Example Org","subscriptionType":"${PLACEHOLDER_PLAN}" }}`);
  assert.equal(redact(out), out);
  assert.equal(redact('{"subscriptionType": null}'), '{"subscriptionType": null}');
});

test("redacts every rate-limit window's utilization, to a share of the same type", () => {
  const line = '{"unifiedWindows":{"seven_day":{"utilization":0.37,"resetsAt":1},"five_hour":{"utilization": 1.2e-1},"opus_week":{"utilization":1}}}';
  const out = redact(line);
  assert.equal(out, `{"unifiedWindows":{"seven_day":{"utilization":${PLACEHOLDER_USAGE},"resetsAt":1},"five_hour":{"utilization": ${PLACEHOLDER_USAGE}},"opus_week":{"utilization":${PLACEHOLDER_USAGE}}}}`);
  assert.equal(typeof JSON.parse(out).unifiedWindows.seven_day.utilization, "number");
  assert.equal(redact('{"utilization":null}'), '{"utilization":null}');
});

test("redacts the pair of ids in a synced plugin's path, in every spelling of its separators", () => {
  const a = "4c1d2e3f-0000-4000-8000-0123456789ab";
  const b = "9a8b7c6d-1111-4111-9111-ba9876543210";
  for (const sep of ["\\", "\\\\", "/"]) {
    const line = `plugins${sep}synced${sep}${a}_${b}${sep}code-review`;
    const out = redact(line);
    assert.equal(out, `plugins${sep}synced${sep}${PLACEHOLDER_UUID}_${PLACEHOLDER_UUID}${sep}code-review`, line);
    assert.equal(redact(out), out);
  }
});

test("names the line and field of each account detail left as recorded, never its value", () => {
  const text = [
    '{"subscriptionType":"Claude Mega","utilization":0.37}',
    `{"subscriptionType":"${PLACEHOLDER_PLAN}","utilization":${PLACEHOLDER_USAGE}}`,
    'plugins\\synced\\4c1d2e3f-0000-4000-8000-0123456789ab_9a8b7c6d-1111-4111-9111-ba9876543210',
  ].join("\n");
  const found = accountDetails(text);
  assert.deepEqual(found, [
    { line: 1, field: "subscriptionType" },
    { line: 1, field: "utilization" },
    { line: 3, field: "synced plugin ids" },
  ]);
  assert.ok(!JSON.stringify(found).includes("Mega") && !JSON.stringify(found).includes("0.37"));
  assert.deepEqual(accountDetails(redact(text)), []);
});

test("a new fixture takes one past the highest number in its folder, whatever the gaps", () => {
  const dir = mkdtempSync(join(tmpdir(), "ersa-number-"));
  try {
    assert.equal(nextFixtureNumber(dir), "00");
    for (const f of ["00-a.jsonl", "00-a.meta.json", "01-b.jsonl", "05-c.jsonl", "notes.txt"]) writeFileSync(join(dir, f), "");
    mkdirSync(join(dir, "record"));
    writeFileSync(join(dir, "record", "40-d.jsonl"), "");
    assert.equal(nextFixtureNumber(dir), "06");
    writeFileSync(join(dir, "99-e.jsonl"), "");
    assert.equal(nextFixtureNumber(dir), "100");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// Values a test invents to look like what a sign-in could leak. None is real.
const FAKE_OAUTH_TOKEN = "sk-ant-oat01-" + "Zq3vXk9TmB2wLp7RcY4nHs8dJf6GaE1u".repeat(3);
const FAKE_JWT = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJlLXBhcnQ";
const FAKE_CODE = "Xk9TmB2wLp7RcY4nHs8dJf6GaE1uZq3v";

test("a token, a key or an authorization code fails the check, which names its line and kind and never its value", () => {
  for (const [line, kind] of [
    [`{"accessToken":"${FAKE_OAUTH_TOKEN}"}`, "an Anthropic key or token"],
    [`stored ${FAKE_OAUTH_TOKEN} for later`, "an Anthropic key or token"],
    ["ANTHROPIC_API_KEY=sk-ant-api03-abcDEF123456", "an Anthropic key or token"],
    [`{"id":"${FAKE_JWT}"}`, "a JSON web token"],
    ["Authorization: Bearer abcdefghij0123", "a bearer token"],
    [`http://localhost:54545/callback?code=${FAKE_CODE.slice(0, 12)}&state=REDACTED`, "a code or token in an address"],
    ["https://x.example/cb#access_token=abc123def456", "a code or token in an address"],
    ['{"refresh_token":"abc123def"}', "a token, key or code field"],
    ['{"authorizationCode": "short-code"}', "a token, key or code field"],
    ['{\\"apiKey\\":\\"abcdefgh\\"}', "a token, key or code field"],
    [`{"code":"${FAKE_CODE}"}`, "a random-looking string"],
    [`paste this: ${FAKE_CODE}`, "a random-looking string"],
  ]) {
    const found = secretsIn(`{"type":"ok"}\n${line}\n`);
    assert.deepEqual([...new Set(found.map((f) => f.line))], [2], line);
    assert.ok(found.some((f) => f.kind === kind), `${line}: ${JSON.stringify(found)}`);
    const said = JSON.stringify(found);
    assert.ok(!said.includes(FAKE_CODE.slice(0, 12)) && !said.includes("abc123") && !said.includes("abcdefg"), "a finding never carries the value");
  }
});

test("the check passes what a recording carries that is not a secret", () => {
  for (const line of [
    '{"type":"assistant","message":{"id":"msg_01AbCdEfGh1234567890XyZaBc","content":[{"type":"tool_use","id":"toolu_01AbCdEfGh1234567890XyZaBcDe"}]}}',
    '{"error":{"request_id":"req_011CVaBcDeFgHiJkLmNoPq12Rs34"}}',
    '{"type":"thinking","signature":"CAQSzgYKEAgTGAI4AUIIdGhpbmtpbmcSDGhAqBpU9xYz1kT3wQ7Lm2VnR8"}',
    '{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk"}}',
    "projects\\\\C--Users-user-AppData-Local-Temp-ersa-rec-AbC123xYz9Q8w7",
    '{"session_id":"1b2c3d4e-0000-4000-8000-123456789abc","sha256":"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"}',
    '{"code":"ENOENT","subtype":"error_during_execution","tokenSource":"none","apiKeySource":"ANTHROPIC_API_KEY"}',
    '{"usage":{"input_tokens":12,"cache_read_input_tokens":0},"tokens":3}',
    "https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&code_challenge=REDACTED&state=REDACTED",
    '{"name":"ReadMcpResourceToolWithLongName","description":"ListMcpResourcesTool returns MCP resources"}',
  ]) {
    assert.deepEqual(secretsIn(line), [], line);
  }
});

// A fixed stream of pseudo-random bytes, so the measure below is the same on every run.
function* seededBytes(seed) {
  let x = seed;
  for (;;) {
    x ^= x << 13;
    x ^= x >>> 17;
    x ^= x << 5;
    x >>>= 0;
    yield x & 0xff;
  }
}

test("the check catches a random token of an unknown kind nearly always", () => {
  const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
  const bytes = seededBytes(633);
  let missed = 0;
  for (let i = 0; i < 2000; i++) {
    let token = "";
    for (let j = 0; j < 32 + (i % 48); j++) token += ALPHABET[bytes.next().value & 63];
    if (secretsIn(`{"value":"${token}"}`).length === 0) missed++;
  }
  assert.ok(missed <= 20, `missed ${missed} of 2000`);
});

test("redacts the one-time values in a sign-in address and keeps its shape", () => {
  const address =
    "https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&scope=user%3Aprofile&code_challenge=Xk9TmB2wLp7RcY4nHs8dJf6GaE1uZq3vAb12Cd34Ef5&code_challenge_method=S256&state=Zq3vXk9TmB2wLp7RcY4nHs8dJf6GaE1uAb12Cd34Ef5&login_hint=jo%40mail.com";
  const out = redact(`{"automaticUrl":"${address}"}`);
  assert.equal(
    out,
    '{"automaticUrl":"https://claude.ai/oauth/authorize?code=true&client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&scope=user%3Aprofile&code_challenge=REDACTED&code_challenge_method=S256&state=REDACTED&login_hint=REDACTED"}',
  );
  assert.equal(redact(out), out);
  assert.deepEqual(secretsIn(out), []);
  // Before redaction the one-time values look random, so the check fails them.
  assert.notDeepEqual(secretsIn(address), []);
  // A nonce, which an OpenID sign-in address carries, in a fragment too.
  assert.equal(redact("https://x.example/a?nonce=Zq3vXk9TmB2wLp7R#nonce=abc"), "https://x.example/a?nonce=REDACTED#nonce=REDACTED");
});

test("redacts an email address written into an address", () => {
  assert.equal(redact("https://x.example/?email=jo.doe%40mail.com&x=1"), "https://x.example/?email=REDACTED&x=1");
  assert.equal(redact("see jo.doe%40mail.com"), `see ${PLACEHOLDER_EMAIL}`);
});

test("replaces the machine's user name as a word, in any case", () => {
  assert.equal(scrubUserName("C:/x/jodoe/y and JoDoe's", "jodoe"), "C:/x/user/y and user's");
  assert.equal(scrubUserName("jodoesmith", "jodoe"), "jodoesmith");
  assert.equal(scrubUserName("al and al", "al"), "al and al", "a name under three letters is left, since it would match too much");
});

function* recordings() {
  // The protocol fixtures, every adapter's tests, where its goldens live, and
  // the Shell's replays, which Core builds from the fixtures.
  const adapters = readdirSync(join(ROOT, "crates"))
    .filter((name) => name.startsWith("ersa-adapter-"))
    .map((name) => join(ROOT, "crates", name, "tests"))
    .filter((dir) => existsSync(dir));
  const dirs = [join(ROOT, "docs", "briefs", "protocol", "fixtures"), ...adapters, join(ROOT, "apps", "shell", "src", "session", "replays")];
  for (const dir of dirs) {
    for (const entry of readdirSync(dir, { recursive: true, withFileTypes: true })) {
      if (entry.isFile()) yield join(entry.parentPath, entry.name);
    }
  }
}

// Every drive-letter path in a recording or golden, JSON-escaped or with
// forward slashes (a lone backslash would also match `d:\n` in prose), and
// every munged path. Both may start only with the placeholders or Windows
// itself. Test sources use invented paths and are not recordings.
const DRIVE_RE = /\b[A-Za-z]:(?:(?:\\\\|\/)[^\\/"'`\s]+)+/g;
const MUNGED_RE = /\b[A-Za-z]--[A-Za-z0-9][A-Za-z0-9-]*/g;
const allowed = (m) => /^C:(?:\\\\|\/)(?:Users(?:\\\\|\/)user|Windows)\b/i.test(m) || /^C--Users-user\b/.test(m);

// A finding names the file, the line and the kind of detail, never the value:
// the guard's output reaches CI's public log.
const lineOf = (text, index) => text.slice(0, index).split("\n").length;
const USED_RE = /"used"\s*:\s*(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;

test("guard: no recording or golden carries a real email, home path, path on the owner's disk, account detail, token, key or code", () => {
  const found = [];
  for (const file of recordings()) {
    const text = readFileSync(file, "utf8");
    const at = (m, kind) => found.push(`${file}:${lineOf(text, m.index)}: ${kind}`);
    for (const m of text.matchAll(EMAIL_RE)) if (m[0] !== PLACEHOLDER_EMAIL) at(m, "an email address");
    for (const re of [HOME_RE, MUNGED_HOME_RE]) {
      for (const m of text.matchAll(re)) if (m.at(-1) !== "user") at(m, "a home folder's user name");
    }
    // Test sources build their own lines, with invented values.
    if (file.endsWith(".rs")) continue;
    for (const re of [DRIVE_RE, MUNGED_RE]) {
      for (const m of text.matchAll(re)) if (!allowed(m[0])) at(m, "a path on the recording machine");
    }
    for (const { line, field } of accountDetails(text)) found.push(`${file}:${line}: ${field} as recorded`);
    for (const { line, kind } of secretsIn(text)) found.push(`${file}:${line}: ${kind}`);
    // Goldens and replays carry a window's share as Ersa projects it, `used`.
    for (const m of text.matchAll(USED_RE)) if (Number(m[1]) !== PLACEHOLDER_USAGE) at(m, "a usage window's share (used) as recorded");
  }
  assert.deepEqual(found, []);
});
