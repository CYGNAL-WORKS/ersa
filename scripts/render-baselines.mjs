#!/usr/bin/env node
// Renders the gallery's screenshot baselines on GitHub for the current branch
// and copies them into apps/shell/e2e/__screenshots__, so no one needs Docker.
//
//   node scripts/render-baselines.mjs
//
// Starts .github/workflows/screenshots.yml on the branch, waits for it, and
// replaces the local baselines with its `screenshots` artifact. Review and
// commit the images yourself. apps/shell/README.md gives its limits.

import { spawnSync } from "node:child_process";
import { cpSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
export const WORKFLOW = "screenshots.yml";
export const BASELINES = join("apps", "shell", "e2e", "__screenshots__");

// `show` passes the command's output through to the terminal, for the long wait.
function defaultRun(cmd, args, { show = false } = {}) {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, stdio: show ? "inherit" : "pipe" });
  if (r.error) return { status: 1, stdout: "", stderr: String(r.error.message) };
  return { status: r.status, stdout: r.stdout ?? "", stderr: r.stderr ?? "" };
}

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
  const remote = must("git", ["ls-remote", "origin", `refs/heads/${branch}`], "Reading the pushed branch").split(/\s+/)[0];
  if (!remote) throw new Error(`${branch} is not pushed; push it, then run this again.`);
  if (remote !== head) throw new Error(`origin/${branch} is at ${remote.slice(0, 7)}, not the local head ${head.slice(0, 7)}; push or pull until they match, then run this again.`);
  // The run renders the pushed head, so uncommitted interface changes would
  // get baselines that do not match them.
  if (must("git", ["status", "--porcelain", "--", "apps/shell"], "Reading the working tree")) {
    throw new Error("apps/shell has uncommitted changes; commit and push them, then run this again.");
  }

  const listRuns = () =>
    JSON.parse(
      must("gh", ["run", "list", "--workflow", WORKFLOW, "--branch", branch, "--event", "workflow_dispatch", "--limit", "20", "--json", "databaseId,headSha,createdAt"], "Listing runs"),
    );
  // Runs that exist before this dispatch are never the one it starts.
  const before = new Set(listRuns().map((r) => r.databaseId));
  must("gh", ["workflow", "run", WORKFLOW, "--ref", branch], `Starting ${WORKFLOW} (it must be on main)`);
  log(`Started ${WORKFLOW} on ${branch} at ${head.slice(0, 7)}.`);

  let id;
  for (let i = 0; i < findTries && !id; i++) {
    if (i) await sleep(2000);
    const mine = listRuns().filter((r) => r.headSha === head && !before.has(r.databaseId)).sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt));
    id = mine[0]?.databaseId;
  }
  if (!id) throw new Error(`No ${WORKFLOW} run for ${head.slice(0, 7)} appeared; look with \`gh run list --workflow ${WORKFLOW}\`.`);

  log(`Waiting for run ${id}.`);
  const watched = run("gh", ["run", "watch", String(id), "--exit-status"], { show: true });
  if (watched.status !== 0) throw new Error(`Run ${id} failed; read it with \`gh run view ${id} --log-failed\`.`);

  const dir = mkdtempSync(join(tmpdir(), "ersa-baselines-"));
  try {
    must("gh", ["run", "download", String(id), "--name", "screenshots", "--dir", dir], `Downloading run ${id}'s screenshots`);
    if (readdirSync(dir).length === 0) throw new Error(`Run ${id}'s screenshots artifact is empty.`);
    const target = join(root, BASELINES);
    rmSync(target, { recursive: true, force: true });
    cpSync(dir, target, { recursive: true });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  log(`Baselines from run ${id} are in ${BASELINES}; review them with git diff, then commit.`);
  return id;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  renderBaselines().catch((e) => {
    process.stderr.write(`${e.message}\n`);
    process.exit(1);
  });
}
