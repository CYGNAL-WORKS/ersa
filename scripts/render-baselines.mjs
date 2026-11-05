#!/usr/bin/env node
// Renders the gallery's screenshot baselines on GitHub for the current branch
// and copies them into apps/shell/e2e/__screenshots__, so no one needs Docker.
//
//   node scripts/render-baselines.mjs
//
// Starts .github/workflows/screenshots.yml on the branch, waits for it, and
// replaces the local baselines with its `screenshots` artifact. Review and
// commit the images yourself. The wait can last half an hour; run it again
// and it waits on the run already going for this head instead of starting
// one. apps/shell/README.md gives its limits.

import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readdirSync, renameSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
const WORKFLOW = "screenshots.yml";
export const BASELINES = join("apps", "shell", "e2e", "__screenshots__");

// Git and gh stall on a credential prompt or a dead network, so prompts are
// off and every command is killed after two minutes and reads as failed. The
// long wait (`show`) has no limit of its own: the workflow's jobs have theirs,
// and its output passes through to the terminal.
const TIMEOUT_MS = 2 * 60 * 1000;
function defaultRun(cmd, args, { show = false } = {}) {
  const r = spawnSync(cmd, args, {
    cwd: ROOT,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    timeout: show ? undefined : TIMEOUT_MS,
    stdio: show ? "inherit" : "pipe",
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0", GH_PROMPT_DISABLED: "1" },
  });
  if (r.error) return { status: 1, stdout: "", stderr: String(r.error.message) };
  return { status: r.status, stdout: r.stdout ?? "", stderr: r.stderr ?? "" };
}

const newestFirst = (a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt);

// Throws an Error whose message says what went wrong and what to do.
export async function renderBaselines({
  run = defaultRun,
  sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
  root = ROOT,
  findTries = 30,
  log = (line) => process.stdout.write(`${line}\n`),
} = {}) {
  const must = (cmd, args, what) => {
    const r = run(cmd, args);
    if (r.status !== 0) throw new Error(`${what} failed: ${(r.stderr || r.stdout).trim()}`);
    return r.stdout.trim();
  };

  const branch = must("git", ["rev-parse", "--abbrev-ref", "HEAD"], "Reading the branch");
  if (branch === "HEAD") throw new Error("HEAD is detached; check out the branch whose baselines to render.");
  const head = must("git", ["rev-parse", "HEAD"], "Reading the head");
  const short = head.slice(0, 7);
  const remote = must("git", ["ls-remote", "origin", `refs/heads/${branch}`], "Reading the pushed branch").split(/\s+/)[0];
  if (!remote) throw new Error(`${branch} is not pushed; push it, then run this again.`);
  if (remote !== head) throw new Error(`origin/${branch} is at ${remote.slice(0, 7)}, not the local head ${short}; push or pull until they match, then run this again.`);
  // The run renders the pushed head, so uncommitted changes would get
  // baselines that do not match them. A previous run's baselines may stay
  // uncommitted: this run replaces them.
  if (must("git", ["status", "--porcelain", "--", ".", `:(exclude)${BASELINES.replaceAll("\\", "/")}`], "Reading the working tree")) {
    throw new Error("The working tree has uncommitted changes; commit and push them, then run this again.");
  }

  const listArgs = ["run", "list", "--workflow", WORKFLOW, "--branch", branch, "--event", "workflow_dispatch", "--limit", "20", "--json", "databaseId,headSha,createdAt,status"];
  const listRuns = () => JSON.parse(must("gh", listArgs, "Listing runs")).filter((r) => r.headSha === head);
  // While polling for the new run, one failed reply only means "not yet".
  const tryListRuns = () => {
    try {
      return listRuns();
    } catch {
      return [];
    }
  };

  // A run already going for this head is waited on, not replaced: a new
  // dispatch would cancel it (the workflow's concurrency group).
  const runs = listRuns();
  let id = runs.filter((r) => r.status !== "completed").sort(newestFirst)[0]?.databaseId;
  if (id) {
    log(`Run ${id} is already rendering ${short}; waiting for it instead of starting another.`);
  } else {
    // Runs that exist before this dispatch are never the one it starts.
    const before = new Set(runs.map((r) => r.databaseId));
    must("gh", ["workflow", "run", WORKFLOW, "--ref", branch], `Starting ${WORKFLOW} (it must be on main)`);
    log(`Started ${WORKFLOW} on ${branch} at ${short}.`);
    for (let i = 0; i < findTries && !id; i++) {
      if (i) await sleep(2000);
      id = tryListRuns().filter((r) => !before.has(r.databaseId)).sort(newestFirst)[0]?.databaseId;
    }
    if (!id) throw new Error(`No ${WORKFLOW} run for ${short} appeared; look with \`gh run list --workflow ${WORKFLOW}\`.`);
  }

  log(`Waiting for run ${id}.`);
  if (run("gh", ["run", "watch", String(id), "--exit-status"], { show: true }).status !== 0) {
    // The watch also exits non-zero when gh loses the connection, so ask
    // GitHub how the run itself ended before calling it failed.
    const view = run("gh", ["run", "view", String(id), "--json", "status,conclusion"]);
    let state;
    try {
      state = view.status === 0 ? JSON.parse(view.stdout) : undefined;
    } catch {}
    if (state?.status !== "completed") throw new Error(`Lost track of run ${id} while it was going; run this again to wait for it.`);
    if (state.conclusion !== "success") throw new Error(`Run ${id} ended ${state.conclusion}; read it with \`gh run view ${id} --log-failed\`.`);
  }

  const dir = mkdtempSync(join(tmpdir(), "ersa-baselines-"));
  try {
    must("gh", ["run", "download", String(id), "--name", "screenshots", "--dir", dir], `Downloading run ${id}'s screenshots`);
    if (readdirSync(dir).length === 0) throw new Error(`Run ${id}'s screenshots artifact is empty.`);
    replace(join(root, BASELINES), dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  log(`Baselines from run ${id} are in ${BASELINES}; review them with git diff, then commit.`);
  return id;
}

// Moves the old baselines aside before copying, and puts them back if the
// copy fails, so a failure never leaves the folder half replaced.
function replace(target, from) {
  const aside = `${target}.old`;
  rmSync(aside, { recursive: true, force: true });
  const had = existsSync(target);
  if (had) renameSync(target, aside);
  try {
    cpSync(from, target, { recursive: true });
  } catch (e) {
    rmSync(target, { recursive: true, force: true });
    if (had) renameSync(aside, target);
    throw new Error(`Copying the baselines failed, and the old ones are back in place: ${e.message}`);
  }
  rmSync(aside, { recursive: true, force: true });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  renderBaselines().catch((e) => {
    process.stderr.write(`${e.message}\n`);
    process.exit(1);
  });
}
