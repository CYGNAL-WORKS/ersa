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
  const line = '{"unifiedWindows":{"seven_day":{"utilization":0.04,"resetsAt":1},"five_hour":{"utilization": 1.2e-1},"opus_week":{"utilization":1}}}';
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
    '{"subscriptionType":"Claude Mega","utilization":0.04}',
    `{"subscriptionType":"${PLACEHOLDER_PLAN}","utilization":${PLACEHOLDER_USAGE}}`,
    'plugins\\synced\\4c1d2e3f-0000-4000-8000-0123456789ab_9a8b7c6d-1111-4111-9111-ba9876543210',
  ].join("\n");
  const found = accountDetails(text);
  assert.deepEqual(found, [
    { line: 1, field: "subscriptionType" },
    { line: 1, field: "utilization" },
    { line: 3, field: "synced plugin ids" },
  ]);
  assert.ok(!JSON.stringify(found).includes("Mega") && !JSON.stringify(found).includes("0.04"));
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

test("guard: no recording or golden carries a real email, home path, path on the owner's disk or account detail", () => {
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
    // Goldens and replays carry a window's share as Ersa projects it, `used`.
    for (const m of text.matchAll(USED_RE)) if (Number(m[1]) !== PLACEHOLDER_USAGE) at(m, "a usage window's share (used) as recorded");
  }
  assert.deepEqual(found, []);
});
