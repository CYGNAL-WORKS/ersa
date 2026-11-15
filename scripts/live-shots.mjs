#!/usr/bin/env node
// Publishes the live checks' screenshots for a pull request body, and prints
// the Markdown that shows them (docs/agents/live-checks.md).
//
//   node scripts/live-shots.mjs
//
// Commits every PNG under apps/shell/live-results, as the last live-check run
// left them, to a branch of its own, `live-shots/<current branch>`, on top of
// that branch's earlier screenshots and without touching the working tree or
// the index, pushes it to origin, and prints one image link per screenshot,
// pinned to the commit. Every earlier commit stays on the branch, so a link in
// an older body keeps working. The repository is public: what you publish,
// anyone can see.
//
// So it publishes only screenshots from checks that ran the stand-in backend.
// A check that ran the real Claude Code can show the owner's account, folders
// or conversation; when any screenshot came from one, or from a check whose
// backend is not recorded, it refuses the whole run and publishes nothing.

import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { defaultRun } from "./render-baselines.mjs";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
export const RESULTS = join("apps", "shell", "live-results");

/**
 * The file the live-check harness writes in each test's results folder, beside its screenshots, naming the backend
 * the test ran: `stand-in` or `claude`. apps/shell/live/harness.ts writes it under the same name.
 */
export const BACKEND_RECORD = "backend.txt";
const STAND_IN = "stand-in";

/**
 * Why each of `shots` (relative to `dir`) may not be published, as `<shot>: <reason>` lines; empty when every one
 * came from a check that ran the stand-in. Fails closed: a missing or unreadable record, or any other backend, refuses.
 */
export function refusals(dir, shots) {
  return shots.flatMap((shot) => {
    let backend;
    try {
      backend = readFileSync(join(dir, dirname(shot), BACKEND_RECORD), "utf8").trim();
    } catch {
      return [`${shot}: no record of the backend it ran`];
    }
    if (backend === STAND_IN) return [];
    return [`${shot}: ran the real backend (${backend || "unnamed"})`];
  });
}

/** `owner/name` of a GitHub remote URL, https or SSH; null for any other. */
export function githubRepo(url) {
  return /github\.com[:/]([^/]+\/[^/]+?)(?:\.git)?\/?$/.exec(url.trim())?.[1] ?? null;
}

/**
 * The PNG files under `dir`, relative to it with forward slashes, in order. Not those in an `attachments` folder:
 * Playwright copies each screenshot a test attaches there, under a hashed name.
 */
export function screenshots(dir) {
  let entries;
  try {
    entries = readdirSync(dir, { recursive: true, withFileTypes: true });
  } catch {
    return [];
  }
  return entries
    .filter((e) => e.isFile() && e.name.endsWith(".png"))
    .map((e) => relative(dir, join(e.parentPath, e.name)).replaceAll("\\", "/"))
    .filter((shot) => !shot.split("/").includes("attachments"))
    .sort();
}

// Throws an Error whose message says what went wrong and what to do; returns the Markdown.
export function publishShots({ root = ROOT, remote = "origin" } = {}) {
  const git = (args, env = {}) => {
    const r = defaultRun("git", args, { cwd: root, env });
    if (r.status !== 0) throw new Error(`git ${args[0]} failed: ${(r.stderr || r.stdout).trim()}`);
    return r.stdout.trim();
  };

  const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]);
  if (branch === "HEAD") throw new Error("HEAD is detached; check out the pull request's branch.");
  const repo = githubRepo(git(["config", "--get", `remote.${remote}.url`]));
  if (!repo) throw new Error(`${remote} is not a GitHub repository, so its images cannot be linked.`);
  const dir = join(root, RESULTS);
  const shots = screenshots(dir);
  if (shots.length === 0) throw new Error(`No screenshots in ${RESULTS}; run the live checks first (pnpm live in apps/shell).`);
  // Before anything is written to git: a refused screenshot must not reach even the local object store.
  const refused = refusals(dir, shots);
  if (refused.length > 0) {
    const folders = [...new Set(refused.map((line) => line.slice(0, line.indexOf(":")).split("/").slice(0, -1).join("/") || "."))];
    throw new Error(
      [
        "Refusing to publish: these screenshots did not come from a check that ran the stand-in backend, and the repository is public.",
        ...refused.map((line) => `  ${line}`),
        `Delete ${folders.map((f) => join(RESULTS, f)).join(", ")}, run this again, and describe what those checks showed in words in the pull request's Evidence.`,
      ].join("\n"),
    );
  }

  // A private index, so the checkout's own index and working tree are untouched.
  const scratch = mkdtempSync(join(tmpdir(), "live-shots-"));
  try {
    const env = { GIT_INDEX_FILE: join(scratch, "index") };
    for (const shot of shots) {
      const blob = git(["hash-object", "-w", join(dir, shot)]);
      git(["update-index", "--add", "--cacheinfo", `100644,${blob},${shot}`], env);
    }
    const tree = git(["write-tree"], env);
    const ref = `refs/heads/live-shots/${branch}`;
    // The branch's tip, if it has one, becomes the parent: a link to an earlier commit must keep resolving.
    const tip = git(["ls-remote", remote, ref]).split(/\s+/)[0];
    if (tip) git(["fetch", "--quiet", remote, ref]);
    const commit = git(["commit-tree", tree, ...(tip ? ["-p", tip] : []), "-m", `Live-check screenshots for ${branch}`]);
    git(["push", remote, `${commit}:${ref}`]);
    return shots.map((shot) => `![${shot}](https://raw.githubusercontent.com/${repo}/${commit}/${shot})`).join("\n");
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    process.stdout.write(`${publishShots()}\n`);
  } catch (e) {
    process.stderr.write(`live-shots: ${e.message}\n`);
    process.exit(1);
  }
}
