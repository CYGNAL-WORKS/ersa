#!/usr/bin/env node
// The merge review: one headless Claude Code run of the review-pr skill on a
// pull request's head once CI passed, posted on the pull request as a review,
// then the head added to the merge queue through scripts/merge-pr.mjs when
// nothing blocks it. Run by scripts/merge-review-watch.mjs on the owner's own
// machine (ADR 0023), with the owner's gh and Claude Code logins, from a
// checkout of the head with origin/main fetched and this script, the gate and
// the hooks taken from main; the same command runs by hand.
//
//   node scripts/merge-review.mjs <n> --head <sha>
//
// GitHub is the only record: each review's body starts with MARK and carries
// its head, verdict and findings in an HTML comment. The first review of a
// pull request reads the whole diff; a later head gets a re-review of the
// commits since the last reviewed head that is its ancestor (ADR 0021). A head
// already reviewed is not reviewed again; a clean one is still enqueued when
// the gate lets it. The run keeps a "Merge review" commit status on the head
// (a person's token may set a status but not create a check run): pending
// while it works or while a clean head waits for the gate, then success,
// failure when findings block the merge, or error when the run itself failed.
// The poller reads that status as the mark that a head has been reviewed, and
// takes up a pending one again after an hour.
//
// Exit 0 when the head was added to the merge queue, is the owner's, waits for
// the gate, or there was nothing to do; 1 when the review blocks the merge;
// 2 on a tool or usage error.

import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Loaded before the review runs, so an edit to the gate's file during it (a
// mutation left behind, a test) changes nothing.
import { MERGE_REVIEW, cleanMergeOfMain, commitsSinceReview, main as gate, pause } from "./merge-pr.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
export const MARK = "<!-- merge-review -->";

// Paths Claude Code reads as instructions, anywhere in the tree: the review
// runs with main's version of each, or none.
export const instructionPaths = (files) => files.filter((f) => /(^|\/)(CLAUDE(\.local)?\.md$|\.claude\/|\.mcp\.json$)/i.test(f));

// Paths whose change the owner merges: the workflows, this review, its gate
// and its poller, the manifest (its --inbox check guards main), the hooks,
// and every instruction file.
export const ownerPaths = (files) => files.filter((f) => /^(\.github\/|scripts\/hooks\/|scripts\/(merge-(pr|review|review-watch)|manifest)\.mjs$)/.test(f) || instructionPaths([f]).length);

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

// The latest review by one of `authors` (the account this runs as) whose head
// is `head` or an ancestor of it. Null when there is none.
export function previousReview(reviews, head, isAncestor, authors) {
  return (
    reviews
      .filter((r) => authors.includes(r.author))
      .map((r) => parseState(r.body))
      .filter((s) => s?.head && (s.head === head || isAncestor(s.head, head)))
      .at(-1) ?? null
  );
}

const RANK = { high: 0, medium: 1, low: 2 };
// "Medium, blocks the merge": the severity, then what blocking() makes of it.
const category = (f) => `${f.severity[0].toUpperCase()}${f.severity.slice(1)}, ${blocking([f]).length ? "blocks the merge" : "does not block"}`;

export function reviewBody({ head, findings, dismissed, outcome, superseded = "" }) {
  const block = blocking(findings);
  // `superseded` names the head the pull request moved to while this review
  // ran: the verdict stands for what it read, but it is not a round.
  const state = JSON.stringify({ head, verdict: block.length ? "blocked" : "clean", findings, ...(superseded ? { superseded } : {}) }).replace(/--/g, "-\\u002d");
  const line = (f) => `- **${f.severity}**${f.origin === "listed" ? " (still open)" : ""}, \`${f.location}\`: ${f.defect} Failure: ${f.scenario}`;
  return [
    MARK,
    `<!-- merge-review ${state} -->`,
    `## Merge review of ${head.slice(0, 7)}`,
    block.length ? `**${block.length} finding${block.length > 1 ? "s" : ""} block${block.length > 1 ? "" : "s"} the merge.**` : "**Nothing blocks the merge.**",
    // Most severe first, each labeled, so a reader who stops at the plain
    // English still knows which findings block (high, medium) and which do not.
    ...[...findings].sort((a, b) => RANK[a.severity] - RANK[b.severity]).map((f) => `- **${category(f)}${f.origin === "listed" ? ", still open" : ""}:** ${f.plain}`),
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
  // Only the blocking ones: a low was reported once, and listing it every
  // round lengthens each review without changing the verdict.
  const listed = blocking(previous.findings).map((f) => `- ${f.severity}, ${f.location}: ${f.defect} Failure: ${f.scenario}`).join("\n");
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

// The review runs the repository's tests, which is running its code as the
// owner, with the owner's gh login in the environment, so the tool list states
// intent rather than a sandbox: no commit, push or write to GitHub. The rules
// are prefixes, so `git -C . push` gets past them: they stop habits, not a
// hostile pull request, which is why a fork is never reviewed. Hooks stay on;
// the settings and the hook scripts are main's.
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

// A stalled git or gh call ends after two minutes; the review itself gets 38,
// the ask for its result 4 and posting the review at most 14 (three posts,
// three reads, two minutes of pauses), so a run ends in about an hour, and a
// run by hand ends too.
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
export function main(argv, { run: r = run, sleep = pause } = {}) {
  try {
    return review(argv, r, sleep);
  } catch (e) {
    return { code: 2, stderr: `${e instanceof ToolError ? e.message : e.stack}\n` };
  }
}

// gh names an app's account `app/<name>` and flags it; the API's own form is `<name>[bot]`.
const isBot = (pr) => pr.author?.is_bot === true || /^app\/|\[bot\]$/.test(pr.author?.login ?? "");

function review(argv, run, sleep) {
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
  const status = commitStatus(run, head);
  // A dependency bump is a policy call (ADR 0015), so a bot's pull request is
  // the owner's to merge, and its third-party code never runs under the review.
  // The status marks the head so the poller does not come back to it.
  if (isBot(pr)) {
    if (!labels.includes("needs-owner")) must("gh", ["pr", "edit", n, "--add-label", "needs-owner"]);
    status("success", "An automated pull request is the owner's to merge; not reviewed.");
    return skip("an automated pull request is the owner's to merge");
  }
  if (must("git", ["rev-parse", "HEAD"]) !== head) fail(`this checkout is not at ${head.slice(0, 7)}`);

  try {
    return decide();
  } catch (e) {
    // The failure that stopped the run is the one to report, even when the
    // status cannot be set either.
    try {
      status("error", `The run failed: ${e.message}`);
    } catch {}
    throw e;
  }

  function decide() {
    status("pending", "Reviewing…");
    const reviews = must("gh", ["api", "--paginate", `repos/{owner}/{repo}/pulls/${n}/reviews?per_page=100`, "--jq", ".[] | {author: .user.login, body}"]).split("\n").filter(Boolean).map((l) => JSON.parse(l));
    // Only reviews this account posted count: a pull request cannot forge one.
    const me = must("gh", ["api", "user", "--jq", ".login"]);
    const isAncestor = (a, b) => run("git", ["merge-base", "--is-ancestor", a, b]).ok;
    const previous = previousReview(reviews, head, isAncestor, [me]);
    const files = must("git", ["diff", "--name-only", "--no-renames", "-z", "origin/main...HEAD"]).split("\0").filter(Boolean);
    // A head whose only commits since the reviewed one merge main without a
    // hand-resolved conflict changes nothing a reviewer read (ADR 0021), so
    // the verdict carries over without a run, as the gate already allows.
    const merged = previous && previous.head !== head ? commitsSinceReview({ reviewed: previous.head, head, git: (a) => run("git", a) }) : [];
    const onlyCleanMerges = merged.length > 0 && merged.every(cleanMergeOfMain);
    const fresh = previous?.head !== head && !onlyCleanMerges;

    let findings = previous?.findings ?? [];
    let dismissed = [];
    if (fresh) {
      instructionsFromMain(files, run);
      ({ findings, dismissed } = ask(run, prompt({ number: n, branch: pr.headRefName, title: pr.title, issue: issueOf(pr.body), head }, previous)));
    }

    const block = blocking(findings);
    const owner = ownerPaths(files);
    let code = 0;
    let outcome;
    // The status the verdict leaves on the head: failure when findings block,
    // success when the head is ready or queued, pending when a clean head
    // waits on something that may change by itself (GitHub still working the
    // merge out, a CI rerun), so the poller tries again when it goes stale.
    let verdict = "success";
    let summary;
    let queue = false;
    if (block.length) [code, verdict, outcome] = [1, "failure", "Fix each blocking finding with the smallest change and push; CI and this review run again."];
    else if (labels.includes("hold")) outcome = "Labeled `hold`, so not added to the merge queue.";
    else {
      // The owner's pull requests go through the gate too, so one with an
      // open inbox entry or unfinished CI waits rather than reading as ready.
      const g = gate([n, "--reviewed", head, "--dry-run"], { run, wait: sleep });
      if (g.code === 0 && owner.length) outcome = `Ready. It changes ${owner.map((p) => `\`${p}\``).join(", ")}, which the owner merges.`;
      else if (g.code === 0) [queue, outcome] = [true, "The merge gate passes; adding it to the merge queue."];
      else if (g.code === 1) [verdict, summary, outcome] = ["pending", `Clean; the merge gate does not pass yet: ${g.reasons.join("; ")}`, `The merge gate does not pass yet; the merge review tries again within the hour, or at once after a push:\n\n${g.stdout}`];
      else [code, verdict, outcome] = [2, "error", `The merge gate could not run:\n\n${g.stderr}`];
    }
    // A review takes 10 to 40 minutes and the head may move meanwhile. The
    // verdict is posted for what it read, marked superseded so the session
    // does not count it as a round, and the old head is not enqueued: the
    // next review reads the new head and carries these findings as listed.
    const current = run("gh", ["pr", "view", n, "--json", "headRefOid", "--jq", ".headRefOid"]);
    const superseded = current.ok && current.out && current.out !== head ? current.out : "";
    if (superseded) {
      queue = false;
      // A gate error is still worth reading; a gate verdict on the old head is not.
      outcome = `${code === 2 ? `${outcome}\n\n` : ""}Superseded: the pull request's head moved to ${superseded.slice(0, 7)} while this review ran, so this is not a round and the gate was not asked. The next review reads the new head; fix what this one names there.`;
    }
    const url = fresh ? post(run, sleep, me, n, head, reviewBody({ head, findings, dismissed, outcome, superseded })) : "";
    if (!block.length && !labels.includes("hold") && owner.length && !labels.includes("needs-owner")) must("gh", ["pr", "edit", n, "--add-label", "needs-owner"]);
    // The enqueue comes before the green status: a run that dies between the
    // two leaves a pending status, which the poller takes up again, rather
    // than a green head that never entered the queue.
    if (queue) {
      const g = gate([n, "--reviewed", head], { run, wait: sleep });
      if (g.code !== 0) fail(`adding it to the merge queue failed after the review was posted: ${g.stdout ?? g.stderr}`);
    }
    status(verdict, summary ?? (block.length ? `${block.length} finding(s) block the merge; see the review.` : outcome.split("\n").filter(Boolean).join(" ")), url);
    return { code, stdout: `#${n} at ${head.slice(0, 7)}: ${block.length ? `${block.length} blocking finding(s)` : "clean"}${fresh ? "" : " (already reviewed)"}. ${outcome}\n` };
  }
}

// The "Merge review" commit status on the head. GitHub keeps 140 characters of
// a description, and each call is a new status with its own time, so a
// pending set again with the gate's reason restarts the poller's hour. A
// status that cannot be set fails the run: the poller reads it as the mark
// that a head is reviewed, so a review without one would run again on the
// next tick.
function commitStatus(run, head) {
  return (state, description, url = "") => {
    const r = run("gh", ["api", `repos/{owner}/{repo}/statuses/${head}`, "-f", `state=${state}`, "-f", `context=${MERGE_REVIEW}`, "-f", `description=${description.slice(0, 140)}`, ...(url ? ["-f", `target_url=${url}`] : [])]);
    if (!r.ok) fail(`the "${MERGE_REVIEW}" status could not be set to ${state}: ${r.err.split("\n")[0]}`);
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

// Posts the review on the head and returns its address, for the status to
// point at; an address GitHub did not give leaves the status without one.
// GitHub answers a post with a 5xx now and then, after the review's whole run
// is spent, so a post that may pass later (a server error, a 403, 408 or 429,
// which is how GitHub answers its rate limits, a dropped connection, a
// timeout) is tried again after a pause, POST_TRIES in all; any other 4xx
// fails at once. A post that failed may still have landed, so after each
// failure and its pause the head's reviews are read, and one of this
// account's with this body counts as posted. A list that cannot be read
// counts as none, so a post that landed may then appear twice, which costs
// a duplicate comment rather than the review.
export const POST_TRIES = 3;
export const POST_PAUSE_MS = 20 * 1000;
function post(run, sleep, me, n, head, body) {
  const dir = mkdtempSync(join(tmpdir(), "merge-review-"));
  const address = (u) => (/^https:\/\//.test(u ?? "") ? u : "");
  try {
    writeFileSync(join(dir, "body.md"), body);
    for (let i = 1; ; i++) {
      const r = run("gh", ["api", `repos/{owner}/{repo}/pulls/${n}/reviews`, "-f", "event=COMMENT", "-f", `commit_id=${head}`, "-F", `body=@${join(dir, "body.md")}`, "--jq", ".html_url"]);
      if (r.ok) return address(r.out);
      if (/HTTP 4(?!03|08|29)\d\d/.test(r.err)) fail(`posting the review failed: ${r.err}`);
      process.stderr.write(`posting the review failed (try ${i} of ${POST_TRIES}): ${r.err}\n`);
      sleep(POST_PAUSE_MS * i);
      const list = run("gh", ["api", "--paginate", `repos/{owner}/{repo}/pulls/${n}/reviews?per_page=100`, "--jq", `.[] | select(.commit_id == "${head}" and .user.login == "${me}") | {body, html_url}`]);
      let landed;
      try {
        landed = list.ok && list.out.split("\n").filter(Boolean).map((l) => JSON.parse(l)).find((x) => x.body === body);
      } catch {}
      if (landed) return address(landed.html_url);
      if (i === POST_TRIES) fail(`posting the review failed ${POST_TRIES} times: ${r.err}`);
    }
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
