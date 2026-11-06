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

import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");
export const RESULTS = join("apps", "shell", "live-results");

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
    const r = spawnSync("git", args, {
      cwd: root,
      encoding: "utf8",
      timeout: 2 * 60 * 1000,
      env: { ...process.env, ...env, GIT_TERMINAL_PROMPT: "0" },
    });
    if (r.status !== 0) throw new Error(`git ${args[0]} failed: ${(r.stderr || r.stdout || String(r.error)).trim()}`);
    return r.stdout.trim();
  };

  const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]);
  if (branch === "HEAD") throw new Error("HEAD is detached; check out the pull request's branch.");
  const repo = githubRepo(git(["config", "--get", `remote.${remote}.url`]));
  if (!repo) throw new Error(`${remote} is not a GitHub repository, so its images cannot be linked.`);
  const dir = join(root, RESULTS);
  const shots = screenshots(dir);
  if (shots.length === 0) throw new Error(`No screenshots in ${RESULTS}; run the live checks first (pnpm live in apps/shell).`);

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
