// Tests for redact.mjs, and a guard over every committed recording and golden
// file. Run: node --test scripts/redact.test.mjs

import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  EMAIL_RE,
  HOME_RE,
  MUNGED_HOME_RE,
  PLACEHOLDER_EMAIL,
  PLACEHOLDER_HOME,
  PLACEHOLDER_MUNGED_REPO,
  PLACEHOLDER_ORG,
  PLACEHOLDER_REPO,
  PLACEHOLDER_UUID,
  REPO,
  expandMcpConfig,
  expandRepo,
  redact,
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
    [`${REPO}2/x ${munged}2`, `${REPO}2/x ${munged}2`],
  ]) {
    assert.equal(redact(input), expected, input);
    assert.equal(redact(expected), expected, expected);
  }
});

test("expands the repository placeholder for a file the CLI reads", () => {
  assert.equal(expandRepo(`{"args":["${PLACEHOLDER_REPO}/scripts/s.mjs"]}`), `{"args":["${REPO}/scripts/s.mjs"]}`);
  const dir = mkdtempSync(join(tmpdir(), "ersa-redact-"));
  try {
    const fixture = join(dir, "fixture.json");
    writeFileSync(fixture, `{"cwd":"${PLACEHOLDER_REPO}"}`);
    const extra = ["--max-turns", "3", "--mcp-config", fixture];
    expandMcpConfig(extra, dir);
    assert.equal(extra[3], join(dir, "mcp-config.json"));
    assert.equal(readFileSync(extra[3], "utf8"), `{"cwd":"${REPO}"}`);
    const none = ["--max-turns", "3"];
    expandMcpConfig(none, dir);
    assert.deepEqual(none, ["--max-turns", "3"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("leaves other paths and placeholders alone", () => {
  const text = `C:\\Windows\\System32 C:\\Users\\Public\\x ${PLACEHOLDER_HOME}/x ${PLACEHOLDER_EMAIL} ${PLACEHOLDER_REPO}/y`;
  assert.equal(redact(text), text);
});

function* recordings() {
  const dirs = [
    join(ROOT, "docs", "briefs", "protocol", "fixtures"),
    join(ROOT, "crates", "ersa-adapter-claude", "tests"),
  ];
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

test("guard: no recording or golden carries a real email, home path or path on the owner's disk", () => {
  const found = [];
  for (const file of recordings()) {
    const text = readFileSync(file, "utf8");
    for (const [m] of text.matchAll(EMAIL_RE)) if (m !== PLACEHOLDER_EMAIL) found.push(`${file}: ${m}`);
    for (const re of [HOME_RE, MUNGED_HOME_RE]) {
      for (const m of text.matchAll(re)) if (m.at(-1) !== "user") found.push(`${file}: ${m[0]}`);
    }
    if (file.endsWith(".rs")) continue;
    for (const re of [DRIVE_RE, MUNGED_RE]) {
      for (const [m] of text.matchAll(re)) if (!allowed(m)) found.push(`${file}: ${m}`);
    }
  }
  assert.deepEqual(found, []);
});
