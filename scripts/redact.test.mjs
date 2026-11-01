// Tests for redact.mjs, and a guard over every committed recording and golden
// file. Run: node --test scripts/redact.test.mjs

import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { EMAIL_RE, HOME_RE, MUNGED_HOME_RE, PLACEHOLDER_EMAIL, PLACEHOLDER_HOME, PLACEHOLDER_ORG, redact } from "./redact.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");

test("redacts the account email and organization", () => {
  const line = '{"account":{"email":"jo.doe@mail.com","organization":"jo.doe@mail.com\'s Organization"}}';
  assert.equal(redact(line), `{"account":{"email":"${PLACEHOLDER_EMAIL}","organization":"${PLACEHOLDER_ORG}"}}`);
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

test("leaves other paths and placeholders alone", () => {
  const text = `C:\\Windows\\System32 C:\\Users\\Public\\x ${PLACEHOLDER_HOME}/x ${PLACEHOLDER_EMAIL}`;
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

test("guard: no recording or golden carries a real email or home path", () => {
  const found = [];
  for (const file of recordings()) {
    const text = readFileSync(file, "utf8");
    for (const [m] of text.matchAll(EMAIL_RE)) if (m !== PLACEHOLDER_EMAIL) found.push(`${file}: ${m}`);
    for (const re of [HOME_RE, MUNGED_HOME_RE]) {
      for (const m of text.matchAll(re)) if (m.at(-1) !== "user") found.push(`${file}: ${m[0]}`);
    }
  }
  assert.deepEqual(found, []);
});
