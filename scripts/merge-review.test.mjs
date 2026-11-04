// Tests for merge-review.mjs, run without GitHub or Claude Code.
// Run: node --test scripts/merge-review.test.mjs

import assert from "node:assert/strict";
import { test } from "node:test";

import { MARK, blocking, claudeArgs, instructionPaths, main, ownerPaths, parseState, previousReview, prompt, reviewBody } from "./merge-review.mjs";

const HEAD = "b".repeat(40);
const OLD = "a".repeat(40);
const finding = (over) => ({ severity: "medium", origin: "new", location: "scripts/x.mjs:3", defect: "drops the last line.", scenario: "a file without a trailing newline loses its last line", plain: "The last line of a file can go missing.", ...over });
const body = (head, findings = []) => reviewBody({ head, findings, dismissed: [], outcome: "" });

test("a high or medium finding blocks, listed or new; lows do not", () => {
  const f = [finding({ severity: "high" }), finding({ severity: "low", origin: "listed" }), finding({ origin: "listed" }), finding({ severity: "low" })];
  assert.deepEqual(blocking(f), [f[0], f[2]]);
  assert.deepEqual(blocking(undefined), []);
});

test("the owner merges workflows, the review, the gate, the hooks and every instruction file", () => {
  const owner = [".github/workflows/review.yml", "scripts/hooks/guard-briefs.mjs", "scripts/merge-pr.mjs", "scripts/merge-review.mjs", "CLAUDE.md", "crates/core/CLAUDE.md", "apps/CLAUDE.local.md", ".claude/skills/x/SKILL.md", "apps/shell/.claude/settings.json", ".mcp.json", "apps/.mcp.json"];
  const others = ["scripts/check.mjs", "scripts/merge-pr.test.mjs", "docs/agents/orchestration.md", "docs/CLAUDE.md.txt", "crates/core/src/lib.rs", "github/x"];
  assert.deepEqual(ownerPaths([...owner, ...others]), owner);
});

test("the instruction files the review takes from main are CLAUDE.md, CLAUDE.local.md, .claude/ and .mcp.json anywhere", () => {
  const files = ["CLAUDE.md", "scripts/CLAUDE.md", "apps/CLAUDE.local.md", "apps/shell/.claude/settings.json", ".claude/agents/x.md", ".mcp.json", "scripts/merge-review.mjs", "docs/NOTCLAUDE.md.txt", ".github/workflows/ci.yml"];
  assert.deepEqual(instructionPaths(files), files.slice(0, 6));
});

test("a review body starts with the marker and carries its state, which round-trips through JSON", () => {
  const findings = [finding({ defect: "writes `a --> b` and `--` into the comment.", plain: "Arrows -- like this --> vanish." }), finding({ severity: "low", origin: "listed" })];
  const text = body(HEAD, findings);
  assert.ok(text.startsWith(`${MARK}\n`));
  // The state comment is one HTML comment: no `--` inside it, so it closes once.
  const state = text.split("\n").find((l) => l.startsWith("<!-- merge-review {"));
  assert.ok(state.endsWith(" -->") && !state.slice(4, -3).includes("--"), state);
  assert.deepEqual(parseState(text), { head: HEAD, verdict: "blocked", findings });
  assert.equal(parseState(body(HEAD)).verdict, "clean");
  assert.match(text, /\*\*1 finding blocks the merge\.\*\*/);
  assert.match(text, /- Arrows -- like this --> vanish\./);
  assert.match(text, /\*\*low\*\* \(still open\), `scripts\/x\.mjs:3`/);
  assert.equal(parseState("<!-- merge-review -->\nno state"), null);
  assert.equal(parseState("a comment that mentions <!-- merge-review -->"), null);
});

test("the previous review is the latest trusted one on this head or an ancestor", () => {
  const isAncestor = (a, b) => a === OLD && b === HEAD;
  const reviews = [
    { author: "github-actions[bot]", body: body("c".repeat(40)) },
    { author: "github-actions[bot]", body: body(OLD, [finding()]) },
    { author: "someone", body: body(HEAD) },
    { author: "github-actions[bot]", body: "Looks good." },
  ];
  assert.deepEqual(previousReview(reviews, HEAD, isAncestor), { head: OLD, verdict: "blocked", findings: [finding()] });
  assert.equal(previousReview(reviews, HEAD, isAncestor, ["github-actions[bot]", "someone"]).head, HEAD, "a run by hand finds its own review");
  assert.equal(previousReview(reviews.slice(0, 1), HEAD, isAncestor), null);
  assert.equal(previousReview([], HEAD, isAncestor), null);
});

test("a first review reads the whole diff; a re-review reads the commits since the previous head", () => {
  const pr = { number: "7", branch: "claude/x", title: "Fix the thing", issue: "5", head: HEAD };
  const first = prompt(pr, null);
  assert.match(first, /steps 1 to 4, on `git diff origin\/main\.\.\.HEAD`/);
  assert.match(first, /fixes issue #5/);
  assert.match(first, /titled "Fix the thing"/);
  assert.doesNotMatch(first, /listed/);
  const again = prompt({ ...pr, issue: undefined }, { head: OLD, findings: [finding()] });
  assert.match(again, /names no issue/);
  assert.match(again, new RegExp(`git log -p --no-merges ${OLD.slice(0, 7)}\\.\\.HEAD --not origin/main`));
  assert.match(again, /- medium, scripts\/x\.mjs:3: drops the last line\./);
  assert.match(again, /step 6/);
  assert.doesNotMatch(again, /steps 1 to 4/);
});

test("the review runs Opus with hooks on and may not commit, push or write on GitHub", () => {
  const args = claudeArgs("p");
  const denied = args.slice(args.indexOf("--disallowedTools") + 1);
  for (const rule of ["Bash(git commit:*)", "Bash(git push:*)", "Bash(gh pr merge:*)", "Bash(gh api:*)", "PowerShell"]) assert.ok(denied.includes(rule), rule);
  assert.ok(!args.includes("--settings"));
  assert.equal(args[args.indexOf("--model") + 1], "opus");
  assert.ok(JSON.parse(args[args.indexOf("--json-schema") + 1]).required.includes("dismissed"));
  assert.deepEqual(claudeArgs("p", "sid").slice(-2), ["--resume", "sid"]);
});

// main() with every tool stood in for. `gh pr view` answers from `pr` for the
// review and from `gate` for the gate (which asks for `mergeable`), the reviews
// list from `reviews`, claude from `claude`, `git diff` from `files` (against
// main) and `since` (against the previous head), and main's tip is an ancestor
// of the head unless `behind`.
function drive(argv, { pr = {}, reviews = [], claude, gate = {}, behind = false, ancestors = [], files = "scripts/x.mjs\0scripts/x.test.mjs\0", since = "", mainHas = [] } = {}) {
  const calls = [];
  const ok = (out = "") => ({ ok: true, out, err: "" });
  const no = (err = "no") => ({ ok: false, out: "", err });
  const run = (cmd, args) => {
    calls.push([cmd, ...args]);
    if (cmd === "gh" && args[1] === "view" && args.some((a) => a.includes("mergeable"))) return gate.view ?? ok(JSON.stringify({ state: "OPEN", isDraft: false, mergeable: "MERGEABLE", headRefOid: HEAD, statusCheckRollup: [{ name: "CI", status: "COMPLETED", conclusion: "SUCCESS" }], ...gate.pr }));
    if (cmd === "gh" && args[1] === "view") return ok(JSON.stringify({ state: "OPEN", isDraft: false, headRefOid: HEAD, headRefName: "claude/x", isCrossRepository: false, labels: [], title: "T", body: "Fixes #5", author: { login: "KPandaK", is_bot: false }, ...pr }));
    if (cmd === "gh" && args[0] === "api" && args[1] === "--paginate") return ok(reviews.map((r) => JSON.stringify(r)).join("\n"));
    if (cmd === "gh" && args[1] === "user") return no();
    if (cmd === "gh" && args[1] === "api" && args[2] === "repos/{owner}/{repo}/check-runs") return ok("77");
    if (cmd === "gh" && args[0] === "pr" && args[1] === "merge") return gate.merge ?? ok();
    if (cmd === "git" && args[0] === "rev-parse") return ok(HEAD);
    if (cmd === "git" && args[0] === "merge-base") return args[2] === "origin/main" ? (behind ? no("") : ok()) : args[2] === HEAD || ancestors.includes(args[2]) ? ok() : no("");
    if (cmd === "git" && args[0] === "diff") return ok(args.includes("origin/main...HEAD") ? files : since);
    if (cmd === "git" && args[0] === "cat-file") return mainHas.includes(args[2].replace("origin/main:", "")) ? ok() : no("");
    if (cmd === "git" && args[0] === "rev-list") return ok("");
    if (cmd === "claude") return ok(JSON.stringify(claude ?? { structured_output: { findings: [], dismissed: [] }, num_turns: 1, total_cost_usd: 0.5 }));
    return ok();
  };
  const got = main(argv, { run });
  const of = (cmd, i, v) => calls.filter((c) => c[0] === cmd && c[i] === v);
  return {
    ...got,
    calls,
    reviewed: of("claude", 0, "claude").length,
    posted: calls.filter((c) => c[0] === "gh" && c[1] === "api" && /\/reviews$/.test(c[2]) && c.includes("event=COMMENT")),
    merged: of("gh", 2, "merge"),
    labeled: of("gh", 2, "edit"),
    checks: calls.filter((c) => c[0] === "gh" && c.some((a) => /check-runs/.test(a))).map((c) => c.find((a) => /^(status=in_progress|conclusion=)/.test(a))),
  };
}

test("a head that moved, a closed, draft or fork pull request gets no review", () => {
  for (const pr of [{ headRefOid: OLD }, { state: "MERGED" }, { isDraft: true }, { isCrossRepository: true }]) {
    const got = drive(["7", "--head", HEAD], { pr });
    assert.equal(got.code, 0, JSON.stringify(pr));
    assert.match(got.stdout, /nothing to do/);
    assert.equal(got.reviewed, 0);
    assert.equal(got.posted.length, 0);
    assert.equal(got.checks.length, 0);
  }
});

test("a clean review is posted on the head and the head merges through the gate, with a check run for the watcher", () => {
  const got = drive(["7", "--head", HEAD]);
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviewed, 1);
  assert.equal(got.posted.length, 1);
  assert.ok(got.posted[0].includes(`commit_id=${HEAD}`));
  assert.deepEqual(got.merged, [["gh", "pr", "merge", "7", "--merge", "--match-head-commit", HEAD]]);
  assert.equal(got.labeled.length, 0);
  assert.deepEqual(got.checks, ["status=in_progress", "conclusion=success"]);
  // The instructions and hooks came from main before the review ran.
  assert.ok(got.calls.some((c) => c.join(" ") === "git checkout origin/main -- CLAUDE.md .claude scripts/hooks"));
  const order = got.calls.map((c) => c.join(" "));
  assert.ok(order.findIndex((c) => c.startsWith("git checkout")) < order.findIndex((c) => c.startsWith("claude")));
});

test("every instruction file the diff touched goes back to main's version, or away where main has none", () => {
  // Paths that exist in no checkout, since the restore deletes from the real tree.
  const files = "scripts/x.mjs\0nope/crates/CLAUDE.md\0nope/apps/.mcp.json\0nope/.claude/settings.json\0";
  const got = drive(["7", "--head", HEAD], { files, mainHas: ["nope/crates/CLAUDE.md", "nope/.claude/settings.json"] });
  const restores = got.calls.filter((c) => c[0] === "git" && c[1] === "checkout").map((c) => c.slice(4).join(" "));
  assert.deepEqual(restores, ["CLAUDE.md .claude scripts/hooks", "nope/crates/CLAUDE.md", "nope/.claude/settings.json"]);
  assert.ok(got.calls.some((c) => c[0] === "git" && c[1] === "cat-file" && c[3] === "origin/main:nope/apps/.mcp.json"));
});

test("a blocking finding is posted, fails the check and exits 1 without merging", () => {
  const got = drive(["7", "--head", HEAD], { claude: { structured_output: { findings: [finding()], dismissed: [{ finding: "x", evidence: "y" }] } } });
  assert.equal(got.code, 1);
  assert.equal(got.posted.length, 1);
  assert.equal(got.merged.length, 0);
  assert.deepEqual(got.checks, ["status=in_progress", "conclusion=failure"]);
});

test("a head already reviewed is not reviewed or posted again, and a clean one still merges", () => {
  const got = drive(["7", "--head", HEAD], { reviews: [{ author: "github-actions[bot]", body: body(HEAD) }] });
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviewed, 0);
  assert.equal(got.posted.length, 0);
  assert.equal(got.merged.length, 1);
  const blocked = drive(["7", "--head", HEAD], { reviews: [{ author: "github-actions[bot]", body: body(HEAD, [finding()]) }] });
  assert.equal(blocked.code, 1);
  assert.equal(blocked.reviewed, 0);
});

test("a previous review on an ancestor makes this a re-review, unless the commits since touched .github/", () => {
  const reviews = [{ author: "github-actions[bot]", body: body(OLD, [finding()]) }];
  const got = drive(["7", "--head", HEAD], { reviews, ancestors: [OLD], since: "scripts/x.mjs\0" });
  assert.equal(got.reviewed, 1);
  const text = (r) => r.calls.find((c) => c[0] === "claude")[2];
  assert.match(text(got), /step 6/);
  assert.match(text(got), /drops the last line/);
  assert.ok(got.calls.some((c) => c[0] === "git" && c[1] === "diff" && c.includes(OLD)));
  // A workflow the pull request carried could have posted that review itself.
  const forged = drive(["7", "--head", HEAD], { reviews, ancestors: [OLD], since: "scripts/x.mjs\0.github/workflows/fake.yml\0" });
  assert.match(text(forged), /steps 1 to 4/);
  assert.doesNotMatch(text(forged), /step 6/);
});

test("a clean head on owner paths, a hold label or a bot's pull request is the owner's and not merged", () => {
  const owner = drive(["7", "--head", HEAD], { files: "scripts/x.mjs\0.github/workflows/review.yml\0" });
  assert.equal(owner.code, 0, owner.stderr);
  assert.equal(owner.merged.length, 0);
  assert.deepEqual(owner.labeled, [["gh", "pr", "edit", "7", "--add-label", "needs-owner"]]);
  assert.match(owner.stdout, /changes `\.github\/workflows\/review\.yml`, which the owner merges/);
  const hold = drive(["7", "--head", HEAD], { pr: { labels: [{ name: "hold" }] }, files: "scripts/merge-pr.mjs\0" });
  assert.equal(hold.code, 0);
  assert.equal(hold.merged.length, 0);
  assert.equal(hold.labeled.length, 0, "a held pull request is not labeled for the owner");
  assert.match(hold.stdout, /hold/);
  for (const author of [{ login: "app/dependabot", is_bot: true }, { login: "dependabot[bot]" }, { login: "renovate", is_bot: true }]) {
    const bot = drive(["7", "--head", HEAD], { pr: { author } });
    assert.equal(bot.merged.length, 0, author.login);
    assert.deepEqual(bot.labeled, [["gh", "pr", "edit", "7", "--add-label", "needs-owner"]]);
  }
  const already = drive(["7", "--head", HEAD], { pr: { author: { login: "app/dependabot", is_bot: true }, labels: [{ name: "needs-owner" }] } });
  assert.equal(already.labeled.length, 0);
});

test("a head behind main is told so and stays green; any other refusal or a gate error is surfaced and red", () => {
  const behind = drive(["7", "--head", HEAD], { behind: true });
  assert.equal(behind.code, 0, behind.stderr);
  assert.match(behind.stdout, /Behind main/);
  assert.equal(behind.merged.length, 0);
  const refused = drive(["7", "--head", HEAD], { behind: true, gate: { pr: { mergeable: "CONFLICTING" } } });
  assert.equal(refused.code, 1);
  assert.match(refused.stdout, /The merge gate refused:[\s\S]*conflicting[\s\S]*main's tip/);
  assert.equal(refused.merged.length, 0);
  assert.equal(refused.posted.length, 1, "the review is kept");
  const broken = drive(["7", "--head", HEAD], { gate: { view: { ok: false, out: "", err: "gh down" } } });
  assert.equal(broken.code, 2);
  assert.match(broken.stdout, /could not run:[\s\S]*gh down/);
  assert.deepEqual(broken.checks, ["status=in_progress", "conclusion=failure"]);
  const failedMerge = drive(["7", "--head", HEAD], { gate: { merge: { ok: false, out: "", err: "head moved" } } });
  assert.equal(failedMerge.code, 2);
  assert.match(failedMerge.stderr, /after the review was posted[\s\S]*head moved/);
});

test("a run that returns no result is resumed once, then fails with nothing posted", () => {
  let n = 0;
  const run = (cmd, args) => {
    if (cmd === "claude") return { ok: true, out: JSON.stringify(++n === 1 ? { session_id: "s1", result: "ran out" } : { is_error: true, subtype: "error_max_turns" }), err: "" };
    if (cmd === "gh" && args[1] === "view") return { ok: true, out: JSON.stringify({ state: "OPEN", isDraft: false, headRefOid: HEAD, headRefName: "b", isCrossRepository: false, labels: [], title: "T", body: "", author: {} }), err: "" };
    if (cmd === "gh" && args[1] === "user") return { ok: false, out: "", err: "" };
    if (cmd === "git" && args[0] === "rev-parse") return { ok: true, out: HEAD, err: "" };
    if (cmd === "gh" && args[0] === "api" && /reviews$/.test(args[1])) throw new Error("posted");
    return { ok: true, out: "", err: "" };
  };
  const got = main(["7", "--head", HEAD], { run });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /no result: error_max_turns/);
  assert.equal(n, 2);
});

test("a usage error or a checkout at another commit exits 2 before anything runs", () => {
  for (const argv of [["7"], ["--head", HEAD], ["7", "--head", HEAD, "extra"], ["7", "x", "y"], ["--head", HEAD, "--head"]]) assert.equal(main(argv, { run: () => assert.fail("ran") }).code, 2, argv.join(" "));
  const run = (cmd, args) => ({ ok: true, out: cmd === "git" && args[0] === "rev-parse" ? OLD : JSON.stringify({ state: "OPEN", isDraft: false, headRefOid: HEAD, isCrossRepository: false, labels: [], author: {} }), err: "" });
  const got = main(["7", "--head", HEAD], { run });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /not at/);
});
