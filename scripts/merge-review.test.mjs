// Tests for merge-review.mjs, run without GitHub or Claude Code.
// Run: node --test scripts/merge-review.test.mjs

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { MARK, POST_PAUSE_MS, POST_TRIES, blocking, claudeArgs, instructionPaths, main, ownerPaths, parseState, previousReview, prompt, reviewBody } from "./merge-review.mjs";

const HEAD = "b".repeat(40);
const OLD = "a".repeat(40);
const ME = "KPandaK";
const finding = (over) => ({ severity: "medium", origin: "new", location: "scripts/x.mjs:3", defect: "drops the last line.", scenario: "a file without a trailing newline loses its last line", plain: "The last line of a file can go missing.", ...over });
const body = (head, findings = []) => reviewBody({ head, findings, dismissed: [], outcome: "" });

test("a high or medium finding blocks, listed or new; lows do not", () => {
  const f = [finding({ severity: "high" }), finding({ severity: "low", origin: "listed" }), finding({ origin: "listed" }), finding({ severity: "low" })];
  assert.deepEqual(blocking(f), [f[0], f[2]]);
  assert.deepEqual(blocking(undefined), []);
});

test("the owner merges workflows, the review, the gate, the poller, the hooks and every instruction file", () => {
  const owner = [".github/workflows/ci.yml", "scripts/hooks/guard-briefs.mjs", "scripts/merge-pr.mjs", "scripts/merge-review.mjs", "scripts/merge-review-watch.mjs", "CLAUDE.md", "crates/core/CLAUDE.md", "apps/CLAUDE.local.md", ".claude/skills/x/SKILL.md", "apps/shell/.claude/settings.json", ".mcp.json", "apps/.mcp.json"];
  const others = ["scripts/check.mjs", "scripts/merge-pr.test.mjs", "scripts/merge-review-watch.test.mjs", "docs/agents/orchestration.md", "docs/CLAUDE.md.txt", "crates/core/src/lib.rs", "github/x"];
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

test("the previous review is the latest one this account posted on this head or an ancestor", () => {
  const isAncestor = (a, b) => a === OLD && b === HEAD;
  const reviews = [
    { author: ME, body: body("c".repeat(40)) },
    { author: ME, body: body(OLD, [finding()]) },
    { author: "someone", body: body(HEAD) },
    { author: "github-actions[bot]", body: body(HEAD) },
    { author: ME, body: "Looks good." },
  ];
  assert.deepEqual(previousReview(reviews, HEAD, isAncestor, [ME]), { head: OLD, verdict: "blocked", findings: [finding()] }, "another account's review, the retired workflow's included, does not count");
  assert.equal(previousReview(reviews, HEAD, isAncestor, [ME, "someone"]).head, HEAD);
  assert.equal(previousReview(reviews.slice(0, 1), HEAD, isAncestor, [ME]), null);
  assert.equal(previousReview([], HEAD, isAncestor, [ME]), null);
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
  const lowOnly = prompt(pr, { head: OLD, findings: [finding({ severity: "low", defect: "a nit." })] });
  assert.doesNotMatch(lowOnly, /a nit/, "a low was reported once and is not listed again");
  assert.match(again, /step 6/);
  assert.doesNotMatch(again, /steps 1 to 4/);
});

test("the review runs Opus with hooks on and may not commit, push or write on GitHub", () => {
  const args = claudeArgs("p");
  const denied = args.slice(args.indexOf("--disallowedTools") + 1);
  for (const rule of ["Bash(git commit:*)", "Bash(git push:*)", "Bash(gh pr merge:*)", "Bash(gh api:*)", "Bash(gh issue reopen:*)", "Bash(gh issue lock:*)", "Bash(gh issue transfer:*)", "PowerShell"]) assert.ok(denied.includes(rule), rule);
  assert.ok(!args.includes("--settings"));
  assert.equal(args[args.indexOf("--model") + 1], "opus");
  assert.ok(JSON.parse(args[args.indexOf("--json-schema") + 1]).required.includes("dismissed"));
  assert.deepEqual(claudeArgs("p", "sid").slice(-2), ["--resume", "sid"]);
});

const REVIEW_URL = "https://github.com/o/r/pull/7#pullrequestreview-1";

// main() with every tool stood in for. `gh pr view` answers from `pr` for the
// review and from `gate` for the gate (which asks for `mergeable`), the reviews
// list from `reviews`, claude from `claude`, `git diff` from `files` (against
// main) and `since` (against the previous head).
function drive(argv, { pr = {}, reviews = [], claude, gate = {}, reviewedMissing = false, ancestors = [], files = "scripts/x.mjs\0scripts/x.test.mjs\0", since = "", mainHas = [], me = { ok: true, out: ME, err: "" }, statusOk = true, revList = "", show = "", posts = [], landed = false } = {}) {
  const calls = [];
  const slept = [];
  const postResults = [...posts];
  const ok = (out = "") => ({ ok: true, out, err: "" });
  const no = (err = "no") => ({ ok: false, out: "", err });
  const run = (cmd, args) => {
    calls.push([cmd, ...args]);
    if (cmd === "gh" && args[1] === "view" && args.some((a) => a.includes("mergeable"))) {
      // The head's checks as GitHub reports them at that moment: CI, and the review's own status as this run last left it.
      const own = calls.filter((c) => c[0] === "gh" && c.some((a) => /\/statuses\//.test(a))).at(-1);
      const review = own ? [{ context: "Merge review", state: own.find((a) => a.startsWith("state=")).slice(6).toUpperCase() }] : [];
      return gate.view ?? ok(JSON.stringify({ state: "OPEN", isDraft: false, mergeable: "MERGEABLE", headRefOid: HEAD, statusCheckRollup: [{ name: "CI", status: "COMPLETED", conclusion: "SUCCESS" }, ...review], ...gate.pr }));
    }
    if (cmd === "gh" && args[1] === "view") return ok(JSON.stringify({ state: "OPEN", isDraft: false, headRefOid: HEAD, headRefName: "claude/x", isCrossRepository: false, labels: [], title: "T", body: "Fixes #5", author: { login: ME, is_bot: false }, ...pr }));
    // The check for a post that failed but landed: the body it sent, when `landed`.
    if (cmd === "gh" && args[0] === "api" && args[1] === "--paginate" && args.at(-1).includes("select(.commit_id")) {
      const sent = calls.filter((c) => c.includes("event=COMMENT")).at(-1);
      return ok(landed && sent ? JSON.stringify({ body: readFileSync(sent.find((a) => a.startsWith("body=@")).slice(6), "utf8"), html_url: REVIEW_URL }) : "");
    }
    if (cmd === "gh" && args[0] === "api" && args[1] === "--paginate") return ok(reviews.map((r) => JSON.stringify(r)).join("\n"));
    if (cmd === "gh" && args[1] === "user") return me;
    if (cmd === "gh" && args[0] === "api" && /\/statuses\//.test(args[1])) return statusOk ? ok(JSON.stringify({ id: 1 })) : no("HTTP 403");
    if (cmd === "gh" && args[0] === "api" && /\/reviews$/.test(args[1])) return postResults.shift() ?? ok(REVIEW_URL);
    if (cmd === "gh" && args[0] === "pr" && args[1] === "merge") return gate.merge ?? ok();
    if (cmd === "git" && args[0] === "rev-parse") return ok(HEAD);
    if (cmd === "git" && args[0] === "merge-base") return (args[2] === HEAD && !reviewedMissing) || ancestors.includes(args[2]) ? ok() : no("");
    if (cmd === "git" && args[0] === "diff") return ok(args.includes("origin/main...HEAD") ? files : since);
    if (cmd === "git" && args[0] === "cat-file") return mainHas.includes(args[2].replace("origin/main:", "")) ? ok() : no("");
    if (cmd === "git" && args[0] === "rev-list") return ok(revList);
    if (cmd === "git" && args[0] === "show") return ok(show);
    if (cmd === "claude") return ok(JSON.stringify(claude ?? { structured_output: { findings: [], dismissed: [] }, num_turns: 1, total_cost_usd: 0.5 }));
    return ok();
  };
  const got = main(argv, { run, sleep: (ms) => slept.push(ms) });
  const of =(cmd, i, v) => calls.filter((c) => c[0] === cmd && c[i] === v);
  return {
    ...got,
    calls,
    slept,
    reviewed:of("claude", 0, "claude").length,
    posted: calls.filter((c) => c[0] === "gh" && c[1] === "api" && /\/reviews$/.test(c[2]) && c.includes("event=COMMENT")),
    queued: of("gh", 2, "merge"),
    labeled: of("gh", 2, "edit"),
    // Each status the run set on the head, as "state" or "state url".
    statuses: calls.filter((c) => c[0] === "gh" && c[1] === "api" && /\/statuses\//.test(c[2])).map((c) => [c.find((a) => a.startsWith("state=")).slice(6), c.find((a) => a.startsWith("target_url="))?.slice(11)].filter(Boolean).join(" ")),
  };
}

test("a head that moved, a closed, draft or fork pull request gets no review and no status", () => {
  for (const pr of [{ headRefOid: OLD }, { state: "MERGED" }, { isDraft: true }, { isCrossRepository: true }]) {
    const got = drive(["7", "--head", HEAD], { pr });
    assert.equal(got.code, 0, JSON.stringify(pr));
    assert.match(got.stdout, /nothing to do/);
    assert.equal(got.reviewed, 0);
    assert.equal(got.posted.length, 0);
    assert.deepEqual(got.statuses, []);
  }
});

test("a clean review is posted on the head, the status points at it, and the head enters the merge queue through the gate", () => {
  const got = drive(["7", "--head", HEAD]);
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviewed, 1);
  assert.equal(got.posted.length, 1);
  assert.ok(got.posted[0].includes(`commit_id=${HEAD}`));
  assert.deepEqual(got.queued, [["gh", "pr", "merge", "7", "--auto", "--merge", "--match-head-commit", HEAD]]);
  assert.equal(got.labeled.length, 0);
  assert.deepEqual(got.statuses, ["pending", `success ${REVIEW_URL}`]);
  const status = got.calls.find((c) => c[0] === "gh" && c.includes("state=success"));
  assert.ok(status.includes("context=Merge review") && status[2] === `repos/{owner}/{repo}/statuses/${HEAD}`, status.join(" "));
  // The instructions and hooks came from main before the review ran.
  assert.ok(got.calls.some((c) => c.join(" ") === "git checkout origin/main -- CLAUDE.md .claude scripts/hooks"));
  const order = got.calls.map((c) => c.join(" "));
  assert.ok(order.findIndex((c) => c.startsWith("git checkout")) < order.findIndex((c) => c.startsWith("claude")));
  // The enqueue comes first: a run that dies between the two leaves a pending head the poller takes up again, not a green one that never queued.
  assert.ok(order.findIndex((c) => c.startsWith("gh pr merge")) < order.findIndex((c) => c.includes("state=success")));
});

test("a post GitHub refuses with a server error is tried again, and the review is not lost", () => {
  const fail502 = { ok: false, out: "", err: "gh: Server Error (HTTP 502)" };
  const got = drive(["7", "--head", HEAD], { posts: [fail502, fail502] });
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviewed, 1, "the review ran once");
  assert.equal(got.posted.length, 3);
  assert.deepEqual(got.slept, [POST_PAUSE_MS, 2 * POST_PAUSE_MS]);
  assert.deepEqual(got.statuses, ["pending", `success ${REVIEW_URL}`]);
});

test("a post that failed but landed is not posted twice", () => {
  const got = drive(["7", "--head", HEAD], { posts: [{ ok: false, out: "", err: "gh: Server Error (HTTP 502)" }], landed: true });
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.posted.length, 1);
  assert.deepEqual(got.statuses, ["pending", `success ${REVIEW_URL}`]);
});

test("a post that fails every try fails the run with an error status", () => {
  const got = drive(["7", "--head", HEAD], { posts: Array(POST_TRIES).fill({ ok: false, out: "", err: "HTTP 502" }) });
  assert.equal(got.code, 2);
  assert.equal(got.posted.length, POST_TRIES);
  assert.match(got.stderr, /posting the review failed 3 times: HTTP 502/);
  assert.deepEqual(got.statuses, ["pending", "error"]);
});

test("every instruction file the diff touched goes back to main's version, or away where main has none", () => {
  const files = "scripts/x.mjs\0nope/crates/CLAUDE.md\0nope/apps/.mcp.json\0nope/.claude/settings.json\0";
  const got = drive(["7", "--head", HEAD], { files, mainHas: ["nope/crates/CLAUDE.md", "nope/.claude/settings.json"] });
  const restores = got.calls.filter((c) => c[0] === "git" && c[1] === "checkout").map((c) => c.slice(4).join(" "));
  assert.deepEqual(restores, ["CLAUDE.md .claude scripts/hooks", "nope/crates/CLAUDE.md", "nope/.claude/settings.json"]);
  const removed = got.calls.filter((c) => c[0] === "git" && c[1] === "rm").map((c) => c.at(-1));
  assert.deepEqual(removed, ["nope/crates/CLAUDE.md", "nope/apps/.mcp.json", "nope/.claude/settings.json"], "each one is deleted first, through git");
  assert.ok(got.calls.some((c) => c[0] === "git" && c[1] === "cat-file" && c[3] === "origin/main:nope/apps/.mcp.json"));
});

test("a blocking finding is posted, fails the status and exits 1 without enqueuing", () => {
  const got = drive(["7", "--head", HEAD], { claude: { structured_output: { findings: [finding()], dismissed: [{ finding: "x", evidence: "y" }] } } });
  assert.equal(got.code, 1);
  assert.equal(got.posted.length, 1);
  assert.equal(got.queued.length, 0);
  assert.deepEqual(got.statuses, ["pending", `failure ${REVIEW_URL}`]);
  const status = got.calls.find((c) => c[0] === "gh" && c.includes("state=failure"));
  assert.ok(status.some((a) => a === "description=1 finding(s) block the merge; see the review."), status.join(" "));
});

test("a head already reviewed is not reviewed or posted again, and a clean one still enters the queue", () => {
  const got = drive(["7", "--head", HEAD], { reviews: [{ author: ME, body: body(HEAD) }] });
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviewed, 0);
  assert.equal(got.posted.length, 0);
  assert.equal(got.queued.length, 1);
  assert.deepEqual(got.statuses, ["pending", "success"], "no review was posted this run, so the status has no address");
  const blocked = drive(["7", "--head", HEAD], { reviews: [{ author: ME, body: body(HEAD, [finding()]) }] });
  assert.equal(blocked.code, 1);
  assert.equal(blocked.reviewed, 0);
  // The retired workflow's review of the same head is another account's and does not count.
  const bot = drive(["7", "--head", HEAD], { reviews: [{ author: "github-actions[bot]", body: body(HEAD) }] });
  assert.equal(bot.reviewed, 1);
});

test("a head that only merged main cleanly since the review carries its verdict without a run", () => {
  const MAIN = "c".repeat(40);
  const reviews = [{ author: ME, body: body(OLD, [finding({ severity: "low" })]) }];
  const merge = `${"d".repeat(40)} ${OLD} ${MAIN}`;
  const got = drive(["7", "--head", HEAD], { reviews, ancestors: [OLD, MAIN], revList: merge });
  assert.equal(got.code, 0, got.stderr);
  assert.equal(got.reviewed, 0);
  assert.equal(got.posted.length, 0);
  assert.equal(got.queued.length, 1);
  assert.match(got.stdout, /already reviewed/);
  // A merge that resolved a conflict by hand, or a merge of another branch, is reviewed.
  const conflict = drive(["7", "--head", HEAD], { reviews, ancestors: [OLD, MAIN], revList: merge, show: "conflict hunk" });
  assert.equal(conflict.reviewed, 1);
  const other = drive(["7", "--head", HEAD], { reviews, ancestors: [OLD], revList: merge });
  assert.equal(other.reviewed, 1);
});

test("a previous review on an ancestor makes this a re-review", () => {
  const reviews = [{ author: ME, body: body(OLD, [finding()]) }];
  const got = drive(["7", "--head", HEAD], { reviews, ancestors: [OLD], since: "scripts/x.mjs\0" });
  assert.equal(got.reviewed, 1);
  const text = got.calls.find((c) => c[0] === "claude")[2];
  assert.match(text, /step 6/);
  assert.match(text, /drops the last line/);
});

test("the account this runs as must be readable: its reviews are the only trusted ones", () => {
  const got = drive(["7", "--head", HEAD], { me: { ok: false, out: "", err: "not logged in" } });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /gh api user.*not logged in/);
  assert.equal(got.reviewed, 0);
  assert.deepEqual(got.statuses, ["pending", "error"]);
});

test("a clean head on owner paths or with a hold label is the owner's and not enqueued; a bot's pull request is the owner's unreviewed, with a status", () => {
  const owner = drive(["7", "--head", HEAD], { files: "scripts/x.mjs\0.github/workflows/ci.yml\0" });
  assert.equal(owner.code, 0, owner.stderr);
  assert.equal(owner.queued.length, 0);
  assert.deepEqual(owner.labeled, [["gh", "pr", "edit", "7", "--add-label", "needs-owner"]]);
  assert.match(owner.stdout, /changes `\.github\/workflows\/ci\.yml`, which the owner merges/);
  assert.deepEqual(owner.statuses, ["pending", `success ${REVIEW_URL}`]);
  // The owner's pull request still goes through the gate: one it refuses (an open inbox entry, CI not done) waits, not ready.
  const waiting = drive(["7", "--head", HEAD], { files: ".github/workflows/ci.yml\0", gate: { pr: { isDraft: true } } });
  assert.equal(waiting.queued.length, 0);
  assert.deepEqual(waiting.statuses, ["pending", `pending ${REVIEW_URL}`]);
  assert.doesNotMatch(waiting.stdout, /which the owner merges/);
  const hold = drive(["7", "--head", HEAD], { pr: { labels: [{ name: "hold" }] }, files: "scripts/merge-pr.mjs\0" });
  assert.equal(hold.code, 0);
  assert.equal(hold.queued.length, 0);
  assert.equal(hold.labeled.length, 0, "a held pull request is not labeled for the owner");
  assert.match(hold.stdout, /hold/);
  for (const author of [{ login: "app/dependabot", is_bot: true }, { login: "dependabot[bot]" }]) {
    const bot = drive(["7", "--head", HEAD], { pr: { author } });
    assert.equal(bot.code, 0, author.login);
    assert.equal(bot.reviewed, 0, "a bot's code never runs under the review");
    assert.equal(bot.queued.length, 0, author.login);
    assert.deepEqual(bot.labeled, [["gh", "pr", "edit", "7", "--add-label", "needs-owner"]]);
    assert.deepEqual(bot.statuses, ["success"], "the status marks the head so the poller does not come back to it");
  }
  const already = drive(["7", "--head", HEAD], { pr: { author: { login: "app/dependabot", is_bot: true }, labels: [{ name: "needs-owner" }] } });
  assert.equal(already.labeled.length, 0);
});

test("a clean head the gate refuses stays pending for another try; a gate error or a failed enqueue is an error", () => {
  const long = { name: "Build, lint, test, dependencies (Linux), and a check whose name goes on for a very long while", status: "COMPLETED", conclusion: "FAILURE" };
  const refused = drive(["7", "--head", HEAD], { gate: { pr: { mergeable: "CONFLICTING", statusCheckRollup: [long, { ...long, name: `${long.name} too` }] } } });
  assert.equal(refused.code, 0, refused.stderr);
  assert.match(refused.stdout, /does not pass yet[\s\S]*conflicting/);
  assert.equal(refused.queued.length, 0);
  assert.equal(refused.posted.length, 1, "the review is kept");
  // Pending twice: once when the run started, once with the reason, so the poller's hour starts from the verdict.
  assert.deepEqual(refused.statuses, ["pending", `pending ${REVIEW_URL}`]);
  const description = refused.calls.filter((c) => c[0] === "gh" && c.includes("state=pending")).at(-1).find((a) => a.startsWith("description=")).slice("description=".length);
  assert.match(description, /^Clean; the merge gate does not pass yet: GitHub reports it conflicting; CI did not pass: Build/);
  assert.equal(description.length, 140, "GitHub keeps 140 characters of a description");
  const broken = drive(["7", "--head", HEAD], { gate: { view: { ok: false, out: "", err: "gh down" } } });
  assert.equal(broken.code, 2);
  assert.match(broken.stdout, /could not run:[\s\S]*gh down/);
  assert.deepEqual(broken.statuses, ["pending", `error ${REVIEW_URL}`]);
  const failedQueue = drive(["7", "--head", HEAD], { gate: { merge: { ok: false, out: "", err: "head moved" } } });
  assert.equal(failedQueue.code, 2);
  assert.match(failedQueue.stderr, /merge queue failed after the review was posted[\s\S]*head moved/);
  assert.deepEqual(failedQueue.statuses, ["pending", "error"], "a failed enqueue is red, never green");
});

test("a status the token cannot set stops the run before the review spends anything", () => {
  const got = drive(["7", "--head", HEAD], { statusOk: false });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /"Merge review" status could not be set to pending: HTTP 403/);
  assert.equal(got.reviewed, 0);
  assert.equal(got.posted.length, 0);
});

test("a run that returns no result is resumed once, briefly, then fails with nothing posted and the status an error", () => {
  let n = 0;
  const statuses = [];
  const minutes = [];
  const run = (cmd, args, m) => {
    if (cmd === "gh" && args[0] === "api" && /\/statuses\//.test(args[1])) {
      statuses.push(args.find((a) => a.startsWith("state=")));
      return { ok: true, out: "{}", err: "" };
    }
    if (cmd === "claude") {
      minutes.push(m);
      return { ok: true, out: JSON.stringify(++n === 1 ? { session_id: "s1", result: "ran out" } : { is_error: true, subtype: "error_max_turns" }), err: "" };
    }
    if (cmd === "gh" && args[1] === "view") return { ok: true, out: JSON.stringify({ state: "OPEN", isDraft: false, headRefOid: HEAD, headRefName: "b", isCrossRepository: false, labels: [], title: "T", body: "", author: {} }), err: "" };
    if (cmd === "gh" && args[1] === "user") return { ok: true, out: ME, err: "" };
    if (cmd === "git" && args[0] === "rev-parse") return { ok: true, out: HEAD, err: "" };
    if (cmd === "gh" && args[0] === "api" && /reviews$/.test(args[1])) throw new Error("posted");
    return { ok: true, out: "", err: "" };
  };
  const got = main(["7", "--head", HEAD], { run });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /no result: error_max_turns/);
  assert.equal(n, 2);
  assert.ok(minutes[0] + minutes[1] < 45, `the two runs end within the hour the poller allows: ${minutes}`);
  assert.deepEqual(statuses, ["state=pending", "state=error"]);
});

test("a usage error or a checkout at another commit exits 2 before anything runs", () => {
  for (const argv of [["7"], ["--head", HEAD], ["7", "--head", HEAD, "extra"], ["7", "x", "y"], ["--head", HEAD, "--head"]]) assert.equal(main(argv, { run: () => assert.fail("ran") }).code, 2, argv.join(" "));
  const run = (cmd, args) => ({ ok: true, out: cmd === "git" && args[0] === "rev-parse" ? OLD : JSON.stringify({ state: "OPEN", isDraft: false, headRefOid: HEAD, isCrossRepository: false, labels: [], author: {} }), err: "" });
  const got = main(["7", "--head", HEAD], { run });
  assert.equal(got.code, 2);
  assert.match(got.stderr, /not at/);
});
