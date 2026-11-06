// Tests for manifest.mjs's inbox switch, the CI step that passes it, and its
// repeated-label check, run on a copy of docs/ so the repository's own generated files are never rewritten.
// Run: node --test scripts/manifest.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { after, test } from "node:test";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const scratch = [];
after(() => scratch.forEach((d) => rmSync(d, { recursive: true, force: true })));

// A copy of docs/ and the two scripts the manifest runs, whose inbox holds
// exactly `entries`: the branch's own inbox, open lines or not, never counts.
function copy(entries = "") {
  const dir = mkdtempSync(join(tmpdir(), "ersa-manifest-"));
  scratch.push(dir);
  cpSync(join(ROOT, "docs"), join(dir, "docs"), { recursive: true });
  mkdirSync(join(dir, "scripts"));
  for (const f of ["manifest.mjs", "merge-pr.mjs"]) cpSync(join(ROOT, "scripts", f), join(dir, "scripts", f));
  writeFileSync(join(dir, "docs", "briefs", "INBOX.md"), `# Inbox\n\n## Open\n\n${entries}`);
  // A manifest that hangs fails this test rather than holding CI until the job's own timeout.
  const run = (...args) => spawnSync(process.execPath, [join(dir, "scripts", "manifest.mjs"), ...args], { encoding: "utf8", timeout: 60_000 });
  run.dir = dir;
  return run;
}

test("an open inbox entry fails the check only with --inbox", () => {
  const run = copy("- [ ] 2026-11-05 none: a gap.\n");
  assert.equal(run("--check").status, 0, "a pull request's CI stays green and its review runs");
  const queue = run("--check", "--inbox");
  assert.equal(queue.status, 1);
  assert.match(queue.stdout, /1 inbox, 1 problems/);
  assert.equal(copy()("--check", "--inbox").status, 0, "an empty inbox passes everywhere");
});

test("CI passes --inbox on the merge queue and main, never on a pull request", () => {
  const ci = readFileSync(join(ROOT, ".github", "workflows", "ci.yml"), "utf8");
  const step = ci.split(/\r?\n/).find((l) => /run: node scripts\/manifest\.mjs --check/.test(l));
  assert.ok(step, "the Manifest step runs the check");
  assert.match(step, /\$\{\{ github\.event_name != 'pull_request' && '--inbox' \|\| '' \}\}/);
  assert.match(ci, /^\s+pull_request:/m);
  assert.match(ci, /^\s+merge_group:/m);
});

test("a bullet label repeated under one section name fails the check", () => {
  const run = copy();
  const brief = join(run.dir, "docs", "briefs", "LAY-SHELL.md");
  const text = readFileSync(brief, "utf8");
  // What a union merge leaves: the old and new wording of one bullet, in a section whose heading also repeats.
  writeFileSync(brief, text.replace("## Development", "- **The foot** says one thing.\n\n## The rail\n\n- **The foot** says another.\n\n## Development"));
  assert.equal(run("--check").status, 1);
  assert.match(readFileSync(join(run.dir, "docs", "MANIFEST.md"), "utf8"), /LAY-SHELL: \*\*The foot\*\* appears twice under "The rail"/);
});
