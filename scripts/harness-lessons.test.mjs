// scripts/harness-lessons.mjs: the per-reviewer count the harness pass starts from.
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";

import { compare, main, mergedPullRequests, report, tally } from "./harness-lessons.mjs";

const dirs = [];
after(() => dirs.forEach((d) => rmSync(d, { recursive: true, force: true })));
const scratch = () => {
  const d = mkdtempSync(join(tmpdir(), "harness-lessons-"));
  dirs.push(d);
  return d;
};

const state = (head, findings, extra = {}) => `<!-- merge-review -->\n<!-- merge-review ${JSON.stringify({ head, verdict: findings.some((f) => f.severity !== "low") ? "blocked" : "clean", findings, ...extra })} -->\n## Merge review`;
const finding = (over) => ({ severity: "medium", origin: "new", location: "apps/shell/src/App.tsx:12", defect: "drops the stop.", scenario: "s", plain: "p", reviewers: ["code"], ...over });
const body = () => "## Summary\n\nS";
const ME = "KPandaK";
const mine = (body) => ({ author: { login: ME }, body });

test("the merged pull requests are the first-parent merges' numbers; other merges carry none", () => {
  const log = ["abc Merge pull request #12 from o/claude/x", "ghi Merge pull request #13 from o/claude/y", "jkl Merge remote-tracking branch 'origin/main' into claude/z", "mno Merge pull request #14 from o/claude/harness-lessons-from-pass"].join("\n");
  assert.deepEqual(mergedPullRequests(log), [12, 13, 14]);
  assert.deepEqual(mergedPullRequests(""), []);
});

test("each blocking new finding by this account is counted against the reviewers that found it; lows, listed copies and other accounts' reviews are left out, and a superseded review's findings count without the round", () => {
  const pr = {
    number: 7,
    title: "T",
    body: body(),
    reviews: [
      mine(state("a".repeat(40), [finding(), finding({ severity: "low", location: "x.css:1" }), finding({ location: "crates/ersa-core/src/send.rs:40", defect: "hangs.", reviewers: ["code", "spec"] })])),
      mine(state("b".repeat(40), [finding({ origin: "listed" })])),
      // A head that moved while its review ran: its finding comes back as listed in the next review.
      mine(state("c".repeat(40), [finding({ location: "crates/ersa-core/src/stale.rs:1", defect: "stale.", reviewers: undefined })], { superseded: "d".repeat(40) })),
      mine(state("d".repeat(40), [finding({ location: "crates/ersa-core/src/stale.rs:1", defect: "stale.", origin: "listed" })])),
      mine(state("e".repeat(40), [])),
      mine("LGTM, a person's review"),
      { author: { login: "someone" }, body: state("f".repeat(40), [finding({ location: "forged/by/another.rs:1", severity: "high" })]) },
      // A state not of the review's shape, whoever posted it, is not a review.
      mine(`<!-- merge-review -->\n<!-- merge-review {"head":null,"findings":[${JSON.stringify(finding())}]} -->`),
      mine(`<!-- merge-review -->\n<!-- merge-review {"head":"${"g".repeat(40)}","verdict":"blocked","findings":{}} -->`),
      mine(`<!-- merge-review -->\n<!-- merge-review {"head":"${"h".repeat(40)}","verdict":"blocked","findings":[null]} -->`),
    ],
  };
  const got = compare(pr, ME);
  assert.equal(got.compared, true);
  assert.equal(got.rounds, 5, "the superseded review is not a round; another account's review and a malformed state are not reviews");
  assert.equal(got.blocked, 4);
  assert.deepEqual(
    got.findings.map((f) => [f.severity, f.location, f.reviewers]),
    [
      ["medium", "apps/shell/src/App.tsx:12", ["code"]],
      ["medium", "crates/ersa-core/src/send.rs:40", ["code", "spec"]],
      ["medium", "crates/ersa-core/src/stale.rs:1", []],
    ],
  );
  assert.deepEqual(tally([got]), [["code", 2], ["spec", 1], ["unnamed", 1]], "a finding two reviewers reported counts for each");
  const text = report([got], ME);
  assert.match(text, /1 pull request\(s\) merged since the last pass; 1 counted, from merge reviews posted by KPandaK; 3 blocking finding\(s\) at merge\./);
  assert.match(text, /Blocking findings by reviewer: code 2, spec 1, unnamed 1\./);
  assert.match(text, /send\.rs:40 \(aaaaaaa\), found by code, spec: hangs\./);
  assert.match(text, /stale\.rs:1 \(ccccccc\), found by unnamed/);
});

test("a pull request with no merge review by this account is reported as not counted", () => {
  assert.deepEqual(compare({ number: 1, title: "A", body: body(), reviews: [mine("LGTM"), { author: { login: "someone" }, body: state("a".repeat(40), []) }] }, ME), { number: 1, title: "A", compared: false, why: "no merge review" });
  assert.match(report([compare({ number: 1, title: "A", body: "", reviews: [] }, ME)], ME), /#1 A: not counted \(no merge review\)\./);
  assert.match(report([], ME), /Blocking findings by reviewer: none\./);
});

test("main finds the last pass, reads each pull request once and keeps the answer, and takes --since", () => {
  const cacheDir = join(scratch(), "cache");
  const calls = [];
  const run = (cmd, args) => {
    calls.push([cmd, ...args]);
    if (cmd === "git" && args.includes("--format=%H %s")) return { ok: true, out: `${"1".repeat(40)} Merge pull request #9 from o/x\n${"2".repeat(40)} Harness: lessons from the #3 to #8 merge reviews\n${"3".repeat(40)} Merge pull request #8 from o/y`, err: "" };
    if (cmd === "git" && args[0] === "log") return { ok: true, out: "abc Merge pull request #9 from o/x\nbcd Merge pull request #10 from o/harness", err: "" };
    if (cmd === "gh" && args[1] === "user") return { ok: true, out: ME, err: "" };
    if (cmd === "gh" && args.includes("10")) return { ok: true, out: JSON.stringify({ number: 10, title: "Harness: lessons from the #3 to #8 merge reviews", body: body(), reviews: [mine(state("f".repeat(40), [finding()]))] }), err: "" };
    if (cmd === "gh") return { ok: true, out: JSON.stringify({ number: 9, title: "Nine", body: body(), reviews: [mine(state("a".repeat(40), [finding()]))] }), err: "" };
    return { ok: false, out: "", err: "unexpected" };
  };
  const first = main([], { run, cacheDir });
  assert.equal(first.code, 0, first.stderr);
  assert.ok(calls.some((c) => c[0] === "git" && c.includes(`${"2".repeat(40)}..origin/main`)), "the merges since the last pass");
  assert.match(first.stdout, /^1 pull request\(s\) merged since the last pass/, "the pass's own pull request is left out by its title");
  assert.match(first.stdout, /#9 Nine: 1 review\(s\), 1 blocked\./);
  assert.doesNotMatch(first.stdout, /#10/);
  assert.match(first.stdout, /App\.tsx:12 \(aaaaaaa\), found by code:/);
  assert.deepEqual(readdirSync(cacheDir), ["10.json", "9.json"]);
  const views = () => calls.filter((c) => c[0] === "gh" && c[1] === "pr").length;
  const before = views();
  const second = main(["--since", "abc"], { run, cacheDir });
  assert.equal(views(), before, "the cache answers the second run");
  assert.ok(calls.some((c) => c[0] === "git" && c.includes("abc..origin/main")));
  assert.match(second.stdout, /#9 Nine/);
  const none = main([], { run: (cmd, args) => (cmd === "git" && args.includes("--format=%H %s") ? { ok: true, out: "x Merge pull request #1 from o/x", err: "" } : { ok: true, out: "", err: "" }), cacheDir });
  assert.equal(none.code, 2);
  assert.match(none.stderr, /pass --since <commit>/);
});

test("a cached file that does not parse is read again from GitHub; a gh answer that is not JSON, or a gh failure, ends the run with its reason", () => {
  const cacheDir = scratch();
  writeFileSync(join(cacheDir, "9.json"), '{"number": 9, "title": "Ni');
  writeFileSync(join(cacheDir, "9.json.tmp"), "[]");
  let answer = { ok: true, out: JSON.stringify({ number: 9, title: "Nine", body: body(), reviews: [] }), err: "" };
  const run = (cmd, args) => {
    if (cmd === "git" && args[0] === "log") return { ok: true, out: "abc Merge pull request #9 from o/x", err: "" };
    if (cmd === "gh" && args[1] === "user") return { ok: true, out: ME, err: "" };
    if (cmd === "gh") return answer;
    return { ok: false, out: "", err: "unexpected" };
  };
  const got = main(["--since", "abc"], { run, cacheDir });
  assert.equal(got.code, 0, got.stderr);
  assert.match(got.stdout, /#9 Nine: not counted \(no merge review\)/);
  assert.deepEqual(JSON.parse(readFileSync(join(cacheDir, "9.json"), "utf8")).title, "Nine", "the torn file is replaced");
  assert.deepEqual(readdirSync(cacheDir), ["9.json"], "no half-written file is left; a stale .tmp is replaced");
  writeFileSync(join(cacheDir, "9.json"), "null");
  assert.equal(main(["--since", "abc"], { run, cacheDir }).code, 0, "a cached value that is not an object is read again");
  rmSync(join(cacheDir, "9.json"));
  answer = { ok: true, out: "", err: "" };
  assert.match(main(["--since", "abc"], { run, cacheDir }).stderr, /gh pr view 9 answered no JSON/);
  answer = { ok: false, out: "", err: "HTTP 403: rate limit" };
  const failed = main(["--since", "abc"], { run, cacheDir });
  assert.equal(failed.code, 2);
  assert.match(failed.stderr, /gh pr view 9 failed: HTTP 403/);
});
