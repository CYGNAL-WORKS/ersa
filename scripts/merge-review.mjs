#!/usr/bin/env node
// The merge review: one headless Claude Code run of the review-pr skill on a
// pull request's head once CI passed, posted on the pull request as a review,
// then the merge through scripts/merge-pr.mjs when nothing blocks it. Run by
// .github/workflows/review.yml on the owner's own machine, from a checkout of
// the head with origin/main fetched and this script, the gate and the hooks
// taken from main; the same command runs by hand.
//
//   node scripts/merge-review.mjs <n> --head <sha>
//
// GitHub is the only record: each review's body starts with MARK and carries
// its head, verdict and findings in an HTML comment. The first review of a
// pull request reads the whole diff; a later head gets a re-review of the
// commits since the last reviewed head that is its ancestor (ADR 0021). A head
// already reviewed is not reviewed again; a clean one is still merged when the
// gate lets it. A workflow_run job's own check lands on main, not on the pull
// request, so the run keeps a "Merge review" check run on the head where it
// can (the Checks API takes the workflow's token, not a person's).
//
// Exit 0 when the head merged, is the owner's, waits for main, or there was
// nothing to do; 1 when the review blocks the merge or the gate refuses; 2 on a
// tool or usage error.

import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Loaded before the review runs, so an edit to the gate's file during it (a
// mutation left behind, a test) changes nothing.
import { BEHIND_MAIN, main as gate } from "./merge-pr.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
export const MARK = "<!-- merge-review -->";
const BOT = "github-actions[bot]";

// Paths Claude Code reads as instructions, anywhere in the tree: the review
// runs with main's version of each, or none.
export const instructionPaths = (files) => files.filter((f) => /(^|\/)(CLAUDE(\.local)?\.md$|\.claude\/|\.mcp\.json$)/i.test(f));

// Paths whose change the owner merges: the workflows, this review and its gate,
// the hooks, and every instruction file.
export const ownerPaths = (files) => files.filter((f) => /^(\.github\/|scripts\/hooks\/|scripts\/merge-(pr|review)\.mjs$)/.test(f) || instructionPaths([f]).length);

// What blocks the merge, listed or new (ADR 0021).
export const blocking = (findings) => (findings ?? []).filter((f) => f.severity !== "low");

export const issueOf = (body) => /\b(?:fixes|closes|resolves)\s+#(\d+)/i.exec(body ?? "")?.[1];

// The state a review body carries: MARK, then one HTML comment holding JSON.
// `--` may not appear inside an HTML comment, so it is escaped within the JSON.
export function parseState(body) {
  const m = /^<!-- merge-review -->\s*<!-- merge-review (.*?) -->/s.exec(body ?? "");
  try {
    return m ? JSON.parse(m[1]) : null;
  } catch {
    return null;
  }
}

// The latest review by a trusted author whose head is `head` or an ancestor of
// it; `authors` is the bot and whoever runs this by hand, so a run by hand
// finds its own reviews. Null when there is none.
export function previousReview(reviews, head, isAncestor, authors = [BOT]) {
  return (
    reviews
      .filter((r) => authors.includes(r.author))
      .map((r) => parseState(r.body))
      .filter((s) => s?.head && (s.head === head || isAncestor(s.head, head)))
      .at(-1) ?? null
  );
}

export function reviewBody({ head, findings, dismissed, outcome }) {
  const block = blocking(findings);
  const state = JSON.stringify({ head, verdict: block.length ? "blocked" : "clean", findings }).replace(/--/g, "-\\u002d");
  const line = (f) => `- **${f.severity}**${f.origin === "listed" ? " (still open)" : ""}, \`${f.location}\`: ${f.defect} Failure: ${f.scenario}`;
  return [
    MARK,
    `<!-- merge-review ${state} -->`,
    `## Merge review of ${head.slice(0, 7)}`,
    block.length ? `**${block.length} finding${block.length > 1 ? "s" : ""} block${block.length > 1 ? "" : "s"} the merge.**` : "**Nothing blocks the merge.**",
    ...findings.map((f) => `- ${f.plain}`),
    outcome,
    "<details><summary>Technical findings and dismissals</summary>\n",
    findings.length ? findings.map(line).join("\n") : "No findings.",
    dismissed.length ? `\nDismissed:\n${dismissed.map((d) => `- ${d.finding} Evidence: ${d.evidence}`).join("\n")}` : "",
    "</details>",
  ].join("\n\n");
}

// The brief for claude -p: a first review of the whole diff, or a re-review of
// the commits since `previous`, a review this run found on an ancestor head.
export function prompt({ number, branch, title, issue, head }, previous) {
  const about = [
    `You are the merge-time reviewer for pull request #${number}, branch \`${branch}\`, titled "${title}", ${issue ? `which fixes issue #${issue}` : "which names no issue"}. This checkout is yours alone and is the pull request's head, ${head.slice(0, 7)}. CLAUDE.md, .claude/ and scripts/hooks/ in it are main's, so the pull request cannot change your instructions; its own versions are at \`git show HEAD:<path>\`.`,
    "Reviewers get that title, never the pull request's description. Mutation runs in a second detached worktree of HEAD outside this checkout, removed when done; restore every file it changes. Fix nothing, commit nothing, push nothing, and write nothing on GitHub.",
  ];
  if (!previous)
    return [
      ...about,
      "Run the review-pr skill at merge time, steps 1 to 4, on `git diff origin/main...HEAD`, with this checkout as the reviewers' checkout.",
      'Return every confirmed finding, low ones included, each with origin "new", and every dismissal with its evidence.',
    ].join("\n\n");
  const range = `${previous.head.slice(0, 7)}..HEAD`;
  const listed = previous.findings.map((f) => `- ${f.severity}, ${f.location}: ${f.defect} Failure: ${f.scenario}`).join("\n");
  return [
    ...about,
    `The merge review of ${previous.head.slice(0, 7)} listed these findings:\n\n${listed || "- none"}`,
    `The commits since are \`git log -p --no-merges ${range} --not origin/main\`, plus \`git show --remerge-diff\` of any merge in that range. Run review-pr step 6 on them (Correctness and Failure; a mechanism they kept, whole) and step 3's mutation on their tests.`,
    'Report each listed finding that is not resolved with origin "listed" and the evidence, and each defect the new commits introduced with origin "new" and a reproduction or the cited line. Do not report code the commits did not touch.',
  ].join("\n\n");
}

const finding = {
  type: "object",
  properties: {
    severity: { enum: ["high", "medium", "low"] },
    origin: { enum: ["listed", "new"], description: "listed: a finding from the previous review, still unresolved; new: one you found" },
    location: { type: "string", description: "file:line" },
    defect: { type: "string" },
    scenario: { type: "string", description: "one-line failure scenario, or the reproduction" },
    plain: { type: "string", description: "For a reader who is not a programmer: one or two sentences on what goes wrong, for whom, and why it matters. No file names or jargon." },
  },
  required: ["severity", "origin", "location", "defect", "scenario", "plain"],
};
const SCHEMA = {
  type: "object",
  properties: {
    findings: { type: "array", items: finding },
    dismissed: { type: "array", items: { type: "object", properties: { finding: { type: "string" }, evidence: { type: "string" } }, required: ["finding", "evidence"] } },
  },
  required: ["findings", "dismissed"],
};

// The review runs the repository's tests, which is running its code with the
// workflow's token in the environment, so the tool list states intent rather
// than a sandbox: no commit, push or write to GitHub. The rules are prefixes,
// so `git -C . push` gets past them: they stop habits, not a hostile pull
// request, which is why a fork is never reviewed. Hooks stay on; the settings
// and the hook scripts are main's.
const NEVER = [
  "PowerShell", "WebFetch", "WebSearch", "Bash(curl:*)", "Bash(git commit:*)", "Bash(git push:*)",
  "Bash(gh pr merge:*)", "Bash(gh pr comment:*)", "Bash(gh pr review:*)", "Bash(gh pr close:*)", "Bash(gh pr reopen:*)", "Bash(gh pr edit:*)", "Bash(gh pr create:*)", "Bash(gh pr ready:*)",
  "Bash(gh issue comment:*)", "Bash(gh issue create:*)", "Bash(gh issue close:*)", "Bash(gh issue edit:*)", "Bash(gh issue reopen:*)", "Bash(gh issue lock:*)", "Bash(gh issue unlock:*)", "Bash(gh issue pin:*)", "Bash(gh issue unpin:*)", "Bash(gh issue transfer:*)", "Bash(gh issue delete:*)", "Bash(gh issue develop:*)", "Bash(gh api:*)",
  "Bash(gh workflow:*)", "Bash(gh run rerun:*)", "Bash(gh run cancel:*)", "Bash(gh release:*)", "Bash(gh repo:*)", "Bash(gh label:*)",
  "Bash(gh secret:*)", "Bash(gh variable:*)", "Bash(gh gist:*)", "Bash(gh auth:*)",
];
export const claudeArgs = (text, resume) => [
  "-p", text, "--model", "opus", "--effort", "medium", "--output-format", "json", "--json-schema", JSON.stringify(SCHEMA),
  "--permission-mode", "acceptEdits", "--strict-mcp-config", "--allowedTools", "Read", "Grep", "Glob", "Agent", "Skill", "Edit", "Write", "Bash",
  "--disallowedTools", ...NEVER, ...(resume ? ["--resume", resume] : []),
];

// A stalled git or gh call ends after two minutes; the review itself gets 38
// and the ask for its result 4, inside the workflow's 45, so the check run is
// finished before the runner kills the job, and a run by hand ends too.
function run(cmd, args, minutes = 2) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, timeout: minutes * 60 * 1000, env: { ...process.env, GIT_TERMINAL_PROMPT: "0", GH_PROMPT_DISABLED: "1" } });
  return { ok: r.status === 0, out: (r.stdout ?? "").trim(), err: [(r.stderr ?? "").trim(), r.error?.message].filter(Boolean).join("; ") };
}

class ToolError extends Error {}
const fail = (m) => {
  throw new ToolError(m);
};

// Runs the review for argv; `run` runs one command (git, gh, claude), so a test
// can stand in for every tool. Returns the exit code and what to print.
export function main(argv, { run: r = run } = {}) {
  try {
    return review(argv, r);
  } catch (e) {
    return { code: 2, stderr: `${e instanceof ToolError ? e.message : e.stack}\n` };
  }
}

// gh names an app's account `app/<name>` and flags it; the API's own form is `<name>[bot]`.
const isBot = (pr) => pr.author?.is_bot === true || /^app\/|\[bot\]$/.test(pr.author?.login ?? "");

function review(argv, run) {
  const at = argv.indexOf("--head");
  const head = at >= 0 ? argv[at + 1] : undefined;
  const n = argv.find((a, i) => i !== at && i !== at + 1 && /^\d+$/.test(a));
  if (!n || !head || argv.length !== 3) fail("usage: node scripts/merge-review.mjs <n> --head <sha>");
  const must = (cmd, args) => {
    const x = run(cmd, args);
    if (!x.ok) fail(`${cmd} ${args.slice(0, 3).join(" ")} failed: ${x.err}`);
    return x.out;
  };
  const skip = (why) => ({ code: 0, stdout: `#${n}: ${why}; nothing to do\n` });

  const pr = JSON.parse(must("gh", ["pr", "view", n, "--json", "state,isDraft,headRefOid,headRefName,isCrossRepository,labels,title,body,author"]));
  if (pr.headRefOid !== head) return skip(`its head is ${pr.headRefOid.slice(0, 7)}, not ${head.slice(0, 7)}`);
  if (pr.state !== "OPEN") return skip(`it is ${pr.state.toLowerCase()}`);
  if (pr.isDraft) return skip("it is a draft");
  if (pr.isCrossRepository) return skip("it is from a fork");
  const labels = pr.labels.map((l) => l.name);
  // A dependency bump is a policy call (ADR 0015), so a bot's pull request is
  // the owner's to merge, and its third-party code never runs under the review.
  if (isBot(pr)) {
    if (!labels.includes("needs-owner")) must("gh", ["pr", "edit", n, "--add-label", "needs-owner"]);
    return skip("an automated pull request is the owner's to merge");
  }
  if (must("git", ["rev-parse", "HEAD"]) !== head) fail(`this checkout is not at ${head.slice(0, 7)}`);

  const reviews = must("gh", ["api", "--paginate", `repos/{owner}/{repo}/pulls/${n}/reviews?per_page=100`, "--jq", ".[] | {author: .user.login, body}"]).split("\n").filter(Boolean).map((l) => JSON.parse(l));
  // The workflow's token cannot read /user, so in the workflow only the bot counts.
  const me = run("gh", ["api", "user", "--jq", ".login"]);
  const isAncestor = (a, b) => run("git", ["merge-base", "--is-ancestor", a, b]).ok;
  const previous = previousReview(reviews, head, isAncestor, me.ok ? [BOT, me.out] : [BOT]);
  const files = must("git", ["diff", "--name-only", "--no-renames", "-z", "origin/main...HEAD"]).split("\0").filter(Boolean);

  const fresh = previous?.head !== head;
  // A re-review reads only the commits since the previous review, unless one
  // of them touched .github/: a workflow a pull request adds can post a review
  // as the bot, so a range that could have forged the previous review gets the
  // whole diff again.
  const since = fresh && previous ? must("git", ["diff", "--name-only", "--no-renames", "-z", previous.head, "HEAD"]).split("\0").filter(Boolean) : [];
  const narrow = previous && !since.some((f) => f.startsWith(".github/")) ? previous : null;
  const check = checkRun(run, head);
  try {
    return decide();
  } catch (e) {
    check("failure", `The run failed: ${e.message}`);
    throw e;
  }

  function decide() {
    let findings = previous?.findings ?? [];
    let dismissed = [];
    if (fresh) {
      instructionsFromMain(files, run);
      ({ findings, dismissed } = ask(run, prompt({ number: n, branch: pr.headRefName, title: pr.title, issue: issueOf(pr.body), head }, narrow)));
    }

    // The check is the review's verdict, finished before the gate reads the
    // head's checks (the gate would otherwise wait for it); anything that then
    // fails the run turns it red.
    const block = blocking(findings);
    check(block.length ? "failure" : "success", block.length ? `${block.length} finding(s) block the merge; see the review.` : "Nothing blocks the merge.");
    const owner = ownerPaths(files);
    const ownerMerges = !block.length && !labels.includes("hold") && owner.length > 0;
    let code = 0;
    let outcome;
    let merge = false;
    if (block.length) [code, outcome] = [1, "Fix each blocking finding with the smallest change and push; CI and this review run again."];
    else if (labels.includes("hold")) outcome = "Labeled `hold`, so not merged.";
    else if (owner.length) outcome = `Ready. It changes ${owner.map((p) => `\`${p}\``).join(", ")}, which the owner merges.`;
    else {
      const g = gate([n, "--reviewed", head, "--dry-run"], { run });
      if (g.code === 0) [merge, outcome] = [true, "The merge gate passes; merging."];
      else if (g.code === 1 && g.reasons.length === 1 && g.reasons[0] === BEHIND_MAIN) outcome = "Behind main: merge main into the branch and push; CI and this review run again, and then it merges.";
      else [code, outcome] = [g.code, `The merge gate ${g.code === 1 ? "refused" : "could not run"}:\n\n${g.stdout ?? g.stderr}`];
    }
    if (fresh) post(run, n, head, reviewBody({ head, findings, dismissed, outcome }));
    if (ownerMerges && !labels.includes("needs-owner")) must("gh", ["pr", "edit", n, "--add-label", "needs-owner"]);
    if (merge) {
      const g = gate([n, "--reviewed", head], { run });
      if (g.code !== 0) fail(g.stderr?.startsWith(`#${n} merged`) ? g.stderr.trimEnd() : `the merge failed after the review was posted: ${g.stdout ?? g.stderr}`);
    }
    if (code) check("failure", outcome);
    return { code, stdout: `#${n} at ${head.slice(0, 7)}: ${block.length ? `${block.length} blocking finding(s)` : "clean"}${fresh ? "" : " (already reviewed)"}. ${outcome}\n` };
  }
}

// A "Merge review" check run on the head, in progress now and finished with
// the review's verdict, so `gh pr checks --watch` waits for it and sees it
// red when findings block; a failure later in the run makes it red too, once.
// Only the workflow's token may create one; a run by hand gets no check and
// says so.
function checkRun(run, head) {
  const url = process.env.GITHUB_RUN_ID ? `${process.env.GITHUB_SERVER_URL}/${process.env.GITHUB_REPOSITORY}/actions/runs/${process.env.GITHUB_RUN_ID}` : "";
  const made = run("gh", ["api", "repos/{owner}/{repo}/check-runs", "-f", "name=Merge review", "-f", `head_sha=${head}`, "-f", "status=in_progress", ...(url ? ["-f", `details_url=${url}`] : []), "--jq", ".id"]);
  if (!made.ok) process.stderr.write(`no check run on the head: ${made.err.split("\n")[0]}\n`);
  let state = "";
  return (conclusion, summary) => {
    if (!made.ok || state === conclusion || state === "failure") return;
    state = conclusion;
    const done = run("gh", ["api", "-X", "PATCH", `repos/{owner}/{repo}/check-runs/${made.out}`, "-f", "status=completed", "-f", `conclusion=${conclusion}`, "-f", "output[title]=Merge review", "-f", `output[summary]=${summary}`]);
    if (!done.ok) process.stderr.write(`the check run was not finished: ${done.err.split("\n")[0]}\n`);
  };
}

// The reviewer's instructions and hooks come from main: the change under review
// cannot rewrite them. Every instruction file the diff touches goes back to
// main's version, or away where main has none.
function instructionsFromMain(files, run) {
  const must = (args) => {
    const x = run("git", args);
    if (!x.ok) fail(`git ${args.join(" ")} failed: ${x.err}`);
  };
  must(["checkout", "origin/main", "--", "CLAUDE.md", ".claude", "scripts/hooks"]);
  for (const p of instructionPaths(files)) {
    must(["rm", "-q", "-r", "-f", "--ignore-unmatch", "--", p]);
    if (run("git", ["cat-file", "-e", `origin/main:${p}`]).ok) must(["checkout", "origin/main", "--", p]);
  }
}

// One claude -p run; a run that ends without its structured result is resumed
// once and asked for it alone. Still none fails the run with claude's stderr.
function ask(run, text) {
  const once = (args, minutes) => {
    const r = run("claude", args, minutes);
    try {
      return JSON.parse(r.out);
    } catch {
      return { is_error: true, subtype: `no JSON result: ${r.err.slice(-600)}` };
    }
  };
  let result = once(claudeArgs(text), 38);
  if (!result.structured_output && result.session_id) result = once(claudeArgs("Return your structured result now. Do no more work.", result.session_id), 4);
  if (!result.structured_output) fail(`claude returned no result: ${result.subtype ?? result.result ?? "error"}`);
  process.stdout.write(`review: ${result.num_turns ?? "?"} turns, $${(result.total_cost_usd ?? 0).toFixed(2)}\n`);
  return result.structured_output;
}

function post(run, n, head, body) {
  const dir = mkdtempSync(join(tmpdir(), "merge-review-"));
  try {
    writeFileSync(join(dir, "body.md"), body);
    const r = run("gh", ["api", `repos/{owner}/{repo}/pulls/${n}/reviews`, "-f", "event=COMMENT", "-f", `commit_id=${head}`, "-F", `body=@${join(dir, "body.md")}`]);
    if (!r.ok) fail(`posting the review failed: ${r.err}`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const { code, stdout, stderr } = main(process.argv.slice(2));
  if (stdout) process.stdout.write(stdout);
  if (stderr) process.stderr.write(stderr);
  process.exit(code);
}
