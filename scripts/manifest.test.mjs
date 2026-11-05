// Tests for manifest.mjs's inbox switch, run on a copy of docs/ so the
// repository's own generated files are never rewritten.
// Run: node --test scripts/manifest.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { appendFileSync, cpSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { after, test } from "node:test";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const scratch = [];
after(() => scratch.forEach((d) => rmSync(d, { recursive: true, force: true })));

// A copy of docs/ and the two scripts the manifest runs, with `entries` appended to the inbox.
function copy(entries = "") {
  const dir = mkdtempSync(join(tmpdir(), "ersa-manifest-"));
  scratch.push(dir);
  cpSync(join(ROOT, "docs"), join(dir, "docs"), { recursive: true });
  mkdirSync(join(dir, "scripts"));
  for (const f of ["manifest.mjs", "merge-pr.mjs"]) cpSync(join(ROOT, "scripts", f), join(dir, "scripts", f));
  appendFileSync(join(dir, "docs", "briefs", "INBOX.md"), entries);
  return (...args) => spawnSync(process.execPath, [join(dir, "scripts", "manifest.mjs"), ...args], { encoding: "utf8" });
}

test("an open inbox entry fails the check only with --inbox, which CI passes on the merge queue and main", () => {
  const run = copy("- [ ] 2026-11-05 none: a gap.\n");
  assert.equal(run("--check").status, 0, "a pull request's CI stays green and its review runs");
  const queue = run("--check", "--inbox");
  assert.equal(queue.status, 1);
  assert.match(queue.stdout, /1 inbox, 1 problems/);
  assert.equal(copy()("--check", "--inbox").status, 0, "an empty inbox passes everywhere");
});
