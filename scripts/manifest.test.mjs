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

// A brief (LAY-SHELL unless named) with `section` inserted before its heading `before`; the check's problems.
function withSection(section, id = "LAY-SHELL", before = "## Development") {
  const run = copy();
  const brief = join(run.dir, "docs", "briefs", `${id}.md`);
  const text = readFileSync(brief, "utf8");
  assert.ok(text.includes(before), `${id} has ${before}`);
  writeFileSync(brief, text.replace(before, `${section}\n\n${before}`));
  const status = run("--check").status;
  const problems = readFileSync(join(run.dir, "docs", "MANIFEST.md"), "utf8").split("## Problems")[1];
  return { status, problems };
}

test("a bullet label repeated under one section name fails the check", () => {
  // What a union merge leaves: the old and new wording of one bullet, in a section whose heading also repeats,
  // a method whose parameters changed, and a nested bullet; a fence between them changes nothing.
  const { status, problems } = withSection(
    "- **The foot** says one thing.\n\n## The rail\n\n```sh\n# a comment\n~~~\n```\n\n- **The foot** says another.\n" +
      "- `a.b { id }`: one.\n- `a.b { id, text }`: two.\n- **P**\n  - **Q** one.\n  - **Q** two.\n" +
      "```js x```\n- **Z** one.\n- **Z** two.",
  );
  assert.equal(status, 1);
  assert.match(problems, /LAY-SHELL: \*\*Z\*\* appears twice under "The rail"/);
  assert.match(problems, /LAY-SHELL: \*\*The foot\*\* appears twice under "The rail"/);
  assert.match(problems, /LAY-SHELL: `a\.b` appears twice under "The rail"/);
  assert.match(problems, /LAY-SHELL: \*\*Q\*\* appears twice under "The rail"/);
});

test("a Built so far section of two lines fails the check; one line passes, fenced code aside", () => {
  // What a union merge of two planning commits that each rewrote the one line leaves: both, adjacent.
  const two = withSection("## Built so far\n\nThe rail lists sessions.\nThe rail lists sessions under their project.");
  assert.equal(two.status, 1);
  assert.match(two.problems, /LAY-SHELL: Built so far holds 2 lines/);
  const one = withSection("## Built so far\n\nThe rail lists sessions under their project; the tree is still to build.\n\n```\ntwo\n\nblocks\n```");
  assert.doesNotMatch(one.problems, /Built so far holds/);
});

test("labels in fenced code, or nested under different bullets, are not repeats", () => {
  const { status, problems } = withSection("## Spare\n\n```\n- **X**\n- **X**\n```\n\n- **P**\n  - **Q** one.\n- **R**\n  - **Q** two.\n- plain\n  - **Q** three.");
  assert.equal(status, 0, problems);
});

test("a fence closes only on a bare marker of its own kind, at least as long", () => {
  // Were a shorter marker, or one with text after it, to close the fence, the bullets would repeat
  // and the last marker would open a fence never closed.
  const { status, problems } = withSection(
    "## Spare\n\n````\n```\n- **Y**\n- **Y**\n````\n\n```\n```js\n- **W**\n- **W**\n```\n\n~~~\n```\n- **V**\n- **V**\n~~~",
  );
  assert.equal(status, 0, problems);
});

test("a fence never closed fails the check", () => {
  const { status, problems } = withSection("## Spare\n\n```\n- **U**\n- **U**");
  assert.equal(status, 1);
  assert.match(problems, /LAY-SHELL: a code fence \(```\) is never closed/);
});

test("a requirement defined inside fenced code still counts", () => {
  const { status, problems } = withSection(
    "```\n- **FLOW-APPROVE-PERMISSION-R01**: a second definition.\n```",
    "FLOW-APPROVE-PERMISSION",
    "## Open questions",
  );
  assert.equal(status, 1);
  assert.match(problems, /FLOW-APPROVE-PERMISSION: requirement FLOW-APPROVE-PERMISSION-R01 is defined 2 times/);
});
