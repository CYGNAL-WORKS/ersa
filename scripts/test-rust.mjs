#!/usr/bin/env node
// Runs the Rust tests with their test binaries side by side. `cargo test` runs
// one binary after another, so a run waits on the sum of them; this builds
// what `cargo test` would build, then runs the binaries a few at a time and
// the doctests beside them. CI's test steps and scripts/check.mjs call it.
//
//   node scripts/test-rust.mjs [cargo test options] [-- test binary options]
//   node scripts/test-rust.mjs --locked                 Core's crates, as CI's Test step
//   node scripts/test-rust.mjs -p ersa-shell --locked   the Shell
//   node scripts/test-rust.mjs --locked -- some_test    only tests whose name holds some_test
//
// Each binary runs as `cargo test` runs it: in its package's folder, with the
// same arguments, its tests on libtest's own threads. Its output is held and
// printed as one block when it ends, so two binaries never interleave. Unlike
// `cargo test`, a failing binary does not stop the others; the run fails at
// the end, naming each binary that failed. Exit 0 when everything passed.

import { spawn } from "node:child_process";
import { closeSync, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));

// Options that pick which targets `cargo test` builds. `--doc` cannot be
// combined with them, so a run that names targets runs no doctests, as
// `cargo test --test x` runs none.
const TARGET_OPTIONS = new Set([
  "--lib", "--bin", "--bins", "--test", "--tests", "--example", "--examples",
  "--bench", "--benches", "--all-targets", "--doc",
]);

/** Splits the command line at `--` into cargo's options and the binaries' options. */
export function splitArgs(argv) {
  const at = argv.indexOf("--");
  return at < 0 ? { cargo: argv, tests: [] } : { cargo: argv.slice(0, at), tests: argv.slice(at + 1) };
}

/**
 * The jobs for one run, from the `compiler-artifact` messages of
 * `cargo test --no-run --message-format=json`: each test executable, in the
 * order `cargo test` runs them (by package; library, then binaries, then
 * integration tests; by name), after the doctests unless targets were named.
 * A test executable is an artifact built in the test profile with an
 * executable; a binary built only so an integration test can launch it
 * (`CARGO_BIN_EXE_*`) is not in the test profile.
 */
export function planJobs(messages, { cargo, tests }) {
  // A library's kind is its crate types (`lib`, `rlib`, `proc-macro`, ...).
  const kindRank = (kinds) => (kinds.includes("test") ? 2 : kinds.includes("bin") ? 1 : 0);
  const binaries = messages
    .filter((m) => m.reason === "compiler-artifact" && m.profile?.test && m.executable)
    .map((m) => ({
      package: m.package_id,
      rank: kindRank(m.target.kind),
      name: m.target.name,
      label: `${m.target.kind.includes("test") ? relative(dirname(m.manifest_path), m.target.src_path) : `unittests ${relative(dirname(m.manifest_path), m.target.src_path)}`} (${relative(ROOT, m.executable)})`,
      cmd: m.executable,
      args: tests,
      // cargo runs a test binary in its package's folder, so tests read
      // their fixtures by a path relative to it.
      cwd: dirname(m.manifest_path),
      env: { CARGO_MANIFEST_DIR: dirname(m.manifest_path) },
    }))
    .sort((a, b) => a.package.localeCompare(b.package) || a.rank - b.rank || a.name.localeCompare(b.name));
  const jobs = binaries.map(({ label, cmd, args, cwd, env }) => ({ label: `Running ${label}`, cmd, args, cwd, env }));
  // Only libraries have doctests, and `cargo test -p <a binary> --doc` fails
  // on a package without one. The doctests go first: rustdoc compiles each
  // one, which then overlaps the binaries.
  const targetsNamed = cargo.some((a) => TARGET_OPTIONS.has(a.split("=")[0]));
  if (!targetsNamed && binaries.some((b) => b.rank === 0)) {
    jobs.unshift({
      label: "Doc-tests",
      cmd: "cargo",
      args: ["test", ...cargo, "--doc", ...(tests.length ? ["--", ...tests] : [])],
      cwd: ROOT,
    });
  }
  return jobs;
}

/** Adds up libtest's `test result:` lines in one job's output. */
export function tally(output) {
  const total = { passed: 0, failed: 0, ignored: 0 };
  // Colors off first: with CARGO_TERM_COLOR=always (CI) the doctests' result
  // word comes wrapped in escape codes.
  const plain = output.replace(/\x1b\[[0-9;]*m/g, "");
  for (const m of plain.matchAll(/test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored;/g)) {
    total.passed += Number(m[1]);
    total.failed += Number(m[2]);
    total.ignored += Number(m[3]);
  }
  return total;
}

/**
 * Runs the jobs at most `size` at a time, in order. Each job's output is
 * held and handed to `write` in one piece when the job ends. Resolves to one
 * result per job, in the jobs' order: its exit status, output and seconds.
 */
export function runJobs(jobs, { size, write, run = spawn }) {
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const index = next++;
      results[index] = await runOne(jobs[index], run);
      write(`\n     ${jobs[index].label}\n${results[index].output}`);
    }
  };
  return Promise.all(Array.from({ length: Math.min(size, jobs.length) }, worker)).then(() => results);
}

function runOne(job, run) {
  const started = Date.now();
  // Stdout and stderr go to one file, as they would to one terminal, so
  // they keep their order: cargo writes its headings to stderr and the
  // results to stdout. Two pipes would lose which came first.
  const dir = mkdtempSync(join(tmpdir(), "ersa-test-rust-"));
  const file = join(dir, "output");
  const fd = openSync(file, "w");
  return new Promise((resolve) => {
    let ended = false;
    const done = (status, extra = "") => {
      if (ended) return;
      ended = true;
      closeSync(fd);
      const output = readFileSync(file, "utf8") + extra;
      rmSync(dir, { recursive: true, force: true });
      resolve({ status, output, seconds: (Date.now() - started) / 1000 });
    };
    const child = run(job.cmd, job.args, { cwd: job.cwd, env: { ...process.env, ...job.env }, stdio: ["ignore", fd, fd] });
    child.on("error", (e) => done(1, `\n${job.cmd} could not start: ${e.message}\n`));
    child.on("close", (code, signal) => done(code ?? 1, signal ? `\nended by ${signal}\n` : ""));
  });
}

/** The closing lines: what ran, what it came to, and each job that failed. */
export function summarise(jobs, results, seconds) {
  const sum = { passed: 0, failed: 0, ignored: 0 };
  const failed = [];
  results.forEach((r, i) => {
    const t = tally(r.output);
    sum.passed += t.passed;
    sum.failed += t.failed;
    sum.ignored += t.ignored;
    if (r.status !== 0) failed.push(`  ${jobs[i].label} (exit ${r.status})`);
  });
  return [
    `\n== ${jobs.length} jobs in ${seconds.toFixed(1)}s: ${sum.passed} passed, ${sum.failed} failed, ${sum.ignored} ignored`,
    ...(failed.length ? ["== failed:", ...failed] : []),
    "",
  ].join("\n");
}

/** Builds with cargo and collects its JSON messages; cargo's own messages go to stderr as usual. */
function build(cargo) {
  return new Promise((resolve) => {
    const child = spawn("cargo", ["test", ...cargo, "--no-run", "--message-format=json-render-diagnostics"], {
      cwd: ROOT,
      stdio: ["ignore", "pipe", "inherit"],
    });
    let text = "";
    child.stdout.on("data", (c) => (text += c));
    child.on("error", (e) => {
      process.stderr.write(`cargo could not start: ${e.message}\n`);
      resolve({ status: 1, messages: [] });
    });
    child.on("close", (code) => {
      const messages = text
        .split("\n")
        .filter((l) => l.startsWith("{"))
        .map((l) => JSON.parse(l));
      resolve({ status: code ?? 1, messages });
    });
  });
}

async function main() {
  const args = splitArgs(process.argv.slice(2));
  const started = Date.now();
  const built = await build(args.cargo);
  if (built.status !== 0) process.exit(built.status);
  const jobs = planJobs(built.messages, args);
  // At most three binaries at once. Each binary already runs its own tests
  // on one thread per logical processor, and a run can be no shorter than
  // its slowest binary (admission's exploration, about three and a half
  // minutes in a debug build); the rest fit in two lines beside it. More at
  // once only adds load, and Core's 10-second budget for its git queries
  // runs out under load: in 28 tests at 24 at a time on a 24-processor
  // machine, in 2 at four at a time.
  const size = Math.min(3, availableParallelism());
  process.stdout.write(`\n== running ${jobs.length} jobs, ${size} at a time\n`);
  const results = await runJobs(jobs, { size, write: (s) => process.stdout.write(s) });
  process.stdout.write(summarise(jobs, results, (Date.now() - started) / 1000));
  process.exit(results.some((r) => r.status !== 0) ? 1 : 0);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
