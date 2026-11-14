#!/usr/bin/env node
// Builds the daily build (ADR 0035): an unsigned Windows installer of
// `origin/main`, made in a checkout of its own so the owner's working trees
// are untouched, and prints the installer's path.
//
//   node scripts/daily-build.mjs            build, then print the installer's path
//   node scripts/daily-build.mjs --install  build, then run the installer
//
// The checkout is a git worktree in %LOCALAPPDATA%\ersa-daily-build, reused
// between runs: each run moves it to the newly fetched `origin/main` and
// removes untracked files, but keeps the ignored build output (target/,
// node_modules/) so a second build is incremental. The script refuses to
// build when that checkout isn't exactly `origin/main`.

import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(fileURLToPath(import.meta.url), "..", "..");

function git(cwd, ...args) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

/** Fetches `origin/main` and puts the checkout at `dir` on it, creating the
 * worktree from `repo` on the first run. Touches no other working tree. */
export function prepareCheckout(repo, dir) {
  // A worktree whose folder was deleted is still registered; forget it so
  // `worktree add` can recreate it.
  git(repo, "worktree", "prune");
  if (!existsSync(dir)) {
    git(repo, "fetch", "--quiet", "origin", "main");
    git(repo, "worktree", "add", "--quiet", "--detach", dir, "origin/main");
    return;
  }
  // Fetch through the checkout itself: it may belong to another clone than
  // the one the script runs from, and its `origin/main` is that clone's ref.
  git(dir, "fetch", "--quiet", "origin", "main");
  git(dir, "checkout", "--quiet", "--detach", "--force", "origin/main");
  // Untracked files go; ignored ones (the build caches) stay.
  git(dir, "clean", "-ffdq");
}

/** Throws unless the checkout at `dir` is exactly `origin/main`: the same
 * commit, with nothing changed or added. */
export function verifyAtMain(dir) {
  const head = git(dir, "rev-parse", "HEAD");
  const main = git(dir, "rev-parse", "origin/main");
  if (head !== main) {
    throw new Error(`The daily build's checkout isn't at origin/main (${head.slice(0, 9)}, not ${main.slice(0, 9)}): ${dir}`);
  }
  const changes = git(dir, "status", "--porcelain");
  if (changes) {
    throw new Error(`The daily build's checkout has changes, so it isn't origin/main: ${dir}\n${changes}`);
  }
}

// Runs a command in `cwd`, showing its output, and throws when it fails.
// pnpm is a .cmd on Windows, which Node starts only through the shell, and
// the shell needs paths with spaces quoted. Cargo's output is pinned to the
// checkout's own target folder, whatever CARGO_TARGET_DIR the owner set, since
// the script reads the build from there.
function run(cwd, cmd, args, targetDir) {
  process.stdout.write(`\n== ${cmd} ${args.join(" ")}\n`);
  const shell = process.platform === "win32";
  const quote = (a) => (shell && /\s/.test(a) ? `"${a}"` : a);
  const env = targetDir ? { ...process.env, CARGO_TARGET_DIR: targetDir } : process.env;
  const result = spawnSync(quote(cmd), args.map(quote), { cwd, stdio: "inherit", shell, env });
  if (result.status !== 0) throw new Error(`${cmd} ${args.join(" ")} failed (exit ${result.status}).`);
}

// Builds the installer in `dir` and returns its path.
export function build(dir) {
  const shell = join(dir, "apps", "shell");
  const target = join(dir, "target");
  const release = join(target, "release");
  const nsis = join(release, "bundle", "nsis");
  // An installer left from an earlier run must never be mistaken for this one.
  rmSync(nsis, { recursive: true, force: true });
  run(shell, "pnpm", ["install", "--frozen-lockfile"]);
  // Core's executable, `ersa-core`, is package ersa-cli's binary.
  run(dir, "cargo", ["build", "--release", "--locked", "-p", "ersa-cli"], target);
  // Tauri bundles an external binary from `<path>-<target triple>.exe` and
  // installs it as `<name>.exe` beside the window, where a release build looks
  // for Core. The setting lives only in this build's extra config: in
  // tauri.conf.json, every build of the Shell would need the file to exist.
  const triple = /^host: (.+)$/m.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))[1];
  const core = join(release, "ersa-core");
  copyFileSync(`${core}.exe`, `${core}-${triple}.exe`);
  const overlay = join(release, "daily-build.tauri.json");
  writeFileSync(overlay, JSON.stringify({ bundle: { externalBin: [core.replaceAll("\\", "/")] } }));
  run(shell, "pnpm", ["exec", "tauri", "build", "--config", overlay], target);

  const installers = existsSync(nsis) ? readdirSync(nsis).filter((f) => f.endsWith("-setup.exe")) : [];
  if (installers.length !== 1) throw new Error(`Expected one installer in ${nsis}, found ${installers.length}.`);
  return join(nsis, installers[0]);
}

function main() {
  if (process.platform !== "win32") {
    throw new Error("The daily build is a Windows installer; run this on Windows.");
  }
  const dir = join(process.env.LOCALAPPDATA, "ersa-daily-build");
  prepareCheckout(ROOT, dir);
  verifyAtMain(dir);
  const installer = build(dir);
  process.stdout.write(`\nInstaller: ${installer}\n`);
  if (process.argv.includes("--install")) run(dir, installer, []);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exit(1);
  }
}
