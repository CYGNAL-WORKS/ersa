// scripts/harness-lessons.mjs: the comparison the harness pass starts from.
import assert from "node:assert/strict";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";

import { compare, main, mergedPullRequests, report, reviewSection } from "./harness-lessons.mjs";

const dirs = [];
after(() => dirs.forEach((d) => rmSync(d, { recursive: true, force: true })));
const scratch = () => {
  const d = mkdtempSync(join(tmpdir(), "harness-lessons-"));
  dirs.push(d);
  return d;
};

const state = (head, findings, extra = {}) => `<!-- merge-review -->\n<!-- merge-review ${JSON.stringify({ head, verdict: findings.some((f) => f.severity !== "low") ? "blocked" : "clean", findings, ...extra })} -->\n## Merge review`;
const finding = (over) => ({ severity: "medium", origin: "new", location: "apps/shell/src/App.tsx:12", defect: "drops the stop.", scenario: "s", plain: "p", ...over });
const body = (review) => `## Summary\n\nS\n\n## Review\n\n${review}\n\n## Inbox entries\n\nNone.`;

test("the merged pull requests are the first-parent merges since the pass, the pass's own left out", () => {
  const log = ["abc Merge pull request #12 from o/claude/x", "def Harness: lessons from the #5 to #11 merge reviews", "ghi Merge pull request #13 from o/claude/y", "jkl Merge remote-tracking branch 'origin/main' into claude/z", "mno Merge pull request #14 from o/claude/harness-lessons-from-pass"].join("\n");
  assert.deepEqual(mergedPullRequests(log), [12, 13, 14]);
  assert.deepEqual(mergedPullRequests(""), []);
});

test("the Review section runs from its heading to the next", () => {
  assert.equal(reviewSection(body("Fixed: a thing.\n\n**Follow-ups:** none.")), "Fixed: a thing.\n\n**Follow-ups:** none.");
  assert.equal(reviewSection("## Summary\n\nno review here"), "");
  assert.equal(reviewSection("## Review\n\nlast section"), "last section");
});

test("each blocking new finding is set against the Review section; lows, listed and superseded ones are left out", () => {
  const pr = {
    number: 7,
    title: "T",
    body: body("Author review ran. Fixed: App.tsx lost the stop on resume (Correctness). Follow-ups: none."),
    reviews: [
      { body: state("a".repeat(40), [finding(), finding({ severity: "low", location: "x.css:1" }), finding({ location: "crates/ersa-core/src/send.rs:40", defect: "hangs." })]) },
      { body: state("b".repeat(40), [finding({ origin: "listed" })]) },
      { body: state("c".repeat(40), [finding({ location: "never/counted.rs:1" })], { superseded: "d".repeat(40) }) },
      { body: state("e".repeat(40), []) },
      { body: "LGTM, a person's review" },
    ],
  };
  const got = compare(pr);
  assert.equal(got.compared, true);
  assert.equal(got.rounds, 3, "the superseded review is not a round");
  assert.equal(got.blocked, 2);
  assert.deepEqual(
    got.findings.map((f) => [f.severity, f.location, f.named]),
    [
      ["medium", "apps/shell/src/App.tsx:12", true],
      ["medium", "crates/ersa-core/src/send.rs:40", false],
    ],
  );
  assert.match(report([got]), /1 pull request\(s\) merged since the last pass; 1 compared; 2 blocking finding\(s\) at merge, 1 not named/);
  assert.match(report([got]), /send\.rs:40 \(aaaaaaa\), NOT NAMED by the author: hangs\./);
});

test("a pull request with no merge review or no Review section is reported as not compared", () => {
  assert.deepEqual(compare({ number: 1, title: "A", body: body("x"), reviews: [{ body: "LGTM" }] }), { number: 1, title: "A", compared: false, why: "no merge review" });
  assert.deepEqual(compare({ number: 2, title: "B", body: "## Summary\n\nonly", reviews: [{ body: state("a".repeat(40), []) }] }), { number: 2, title: "B", compared: false, why: "no Review section in the body" });
  assert.match(report([compare({ number: 1, title: "A", body: "", reviews: [] })]), /#1 A: not compared \(no merge review\)\./);
});

test("main finds the last pass, reads each pull request once and keeps the answer, and takes --since and --json", () => {
  const cacheDir = join(scratch(), "cache");
  const calls = [];
  const run = (cmd, args) => {
    calls.push([cmd, ...args]);
    if (cmd === "git" && args.includes("--format=%H %s")) return { ok: true, out: `${"1".repeat(40)} Merge pull request #9 from o/x\n${"2".repeat(40)} Harness: lessons from the #3 to #8 merge reviews\n${"3".repeat(40)} Merge pull request #8 from o/y`, err: "" };
    if (cmd === "git" && args[0] === "log") return { ok: true, out: "abc Merge pull request #9 from o/x", err: "" };
    if (cmd === "gh") return { ok: true, out: JSON.stringify({ number: 9, title: "Nine", body: body("Fixed: App.tsx."), reviews: [{ body: state("a".repeat(40), [finding()]) }] }), err: "" };
    return { ok: false, out: "", err: "unexpected" };
  };
  const first = main([], { run, cacheDir });
  assert.equal(first.code, 0, first.stderr);
  assert.ok(calls.some((c) => c[0] === "git" && c.includes(`${"2".repeat(40)}..origin/main`)), "the merges since the last pass");
  assert.match(first.stdout, /#9 Nine: 1 review\(s\), 1 blocked\./);
  assert.match(first.stdout, /App\.tsx:12 \(aaaaaaa\), named by the author/);
  assert.deepEqual(readdirSync(cacheDir), ["9.json"]);
  const ghCalls = () => calls.filter((c) => c[0] === "gh").length;
  const before = ghCalls();
  const second = main(["--json", "--since", "abc"], { run, cacheDir });
  assert.equal(ghCalls(), before, "the cache answers the second run");
  assert.ok(calls.some((c) => c[0] === "git" && c.includes("abc..origin/main")));
  assert.equal(JSON.parse(second.stdout)[0].number, 9);
  const none = main([], { run: (cmd, args) => (cmd === "git" && args.includes("--format=%H %s") ? { ok: true, out: "x Merge pull request #1 from o/x", err: "" } : { ok: true, out: "", err: "" }), cacheDir });
  assert.equal(none.code, 2);
  assert.match(none.stderr, /pass --since <commit>/);
});
