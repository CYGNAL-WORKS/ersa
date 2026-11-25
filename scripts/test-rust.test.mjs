// Tests for test-rust.mjs, run without cargo: Node stands in for the test binaries.
// Run: node --test scripts/test-rust.test.mjs

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { planJobs, runJobs, splitArgs, stillRunning, summarise, tally, testRust } from "./test-rust.mjs";

// A compiler-artifact message as `cargo test --no-run --message-format=json` prints it.
// `dir` is the package's folder; `executable` the path cargo reports.
function artifact(pkg, kind, name, options = {}) {
  const { test = true, src = `src/${name}.rs`, dir = join("/repo/crates", pkg) } = options;
  const executable = options.executable ?? (test || kind.includes("bin") ? join("/repo/target", `${name}-${pkg}`) : null);
  return {
    reason: "compiler-artifact",
    package_id: `path+file:///repo/crates/${pkg}#0.1.0`,
    manifest_path: join(dir, "Cargo.toml"),
    target: { kind, name, src_path: join(dir, src) },
    profile: { test },
    executable,
  };
}

const messages = [
  { reason: "build-finished", success: true },
  artifact("core", ["test"], "zeta", { src: "tests/zeta.rs" }),
  artifact("core", ["lib"], "core"),
  artifact("adapter", ["test"], "alpha", { src: "tests/alpha.rs" }),
  // Built so an integration test can launch it, not a test binary.
  artifact("core", ["bin"], "core-bin", { test: false }),
  artifact("adapter", ["rlib"], "adapter"),
  artifact("core", ["bin"], "core-bin"),
];

test("the command line splits at -- into cargo's options and the binaries'", () => {
  assert.deepEqual(splitArgs(["--locked", "-p", "x", "--", "name", "--nocapture"]), {
    cargo: ["--locked", "-p", "x"],
    tests: ["name", "--nocapture"],
  });
  assert.deepEqual(splitArgs(["--locked"]), { cargo: ["--locked"], tests: [] });
});

test("every test executable runs once, in cargo's order, after the doctests", () => {
  const jobs = planJobs(messages, { cargo: ["--locked"], tests: [] });
  assert.deepEqual(
    jobs.map((j) => j.label.split(" (")[0]),
    [
      "Doc-tests",
      `Running unittests ${join("src", "adapter.rs")}`,
      `Running ${join("tests", "alpha.rs")}`,
      `Running unittests ${join("src", "core.rs")}`,
      `Running unittests ${join("src", "core-bin.rs")}`,
      `Running ${join("tests", "zeta.rs")}`,
    ],
  );
  assert.deepEqual(jobs[0].args, ["test", "--locked", "--doc"]);
});

test("each binary runs in its package's folder with the binaries' options", () => {
  const jobs = planJobs(messages, { cargo: [], tests: ["name"] });
  const zeta = jobs.find((j) => j.label.includes("zeta"));
  assert.equal(zeta.cwd, join("/repo/crates", "core"));
  assert.equal(zeta.env.CARGO_MANIFEST_DIR, join("/repo/crates", "core"));
  assert.deepEqual(zeta.args, ["name"]);
  assert.deepEqual(jobs[0].args, ["test", "--doc", "--", "name"]);
});

test("no doctests when targets are named or nothing selected is a library", () => {
  for (const cargo of [["--test", "zeta"], ["--lib"], ["--test=zeta"]]) {
    assert.ok(!planJobs(messages, { cargo, tests: [] }).some((j) => j.label === "Doc-tests"), cargo.join(" "));
  }
  const binOnly = [artifact("cli", ["bin"], "cli"), artifact("cli", ["test"], "replay", { src: "tests/replay.rs" })];
  assert.ok(!planJobs(binOnly, { cargo: [], tests: [] }).some((j) => j.label === "Doc-tests"));
});

// A job that prints `lines` with a pause between them, then exits with `status`.
// The script is fixed; what it prints and how reach it as arguments, never as code.
function node(label, lines, status = 0, pauseMs = 30) {
  const script = `const [l, status, pause] = [JSON.parse(process.argv[1]), Number(process.argv[2]), Number(process.argv[3])];let i=0;const t=setInterval(()=>{if(i<l.length){process.stdout.write(l[i++]+"\\n")}else{clearInterval(t);process.exit(status)}},pause);`;
  return { label, cmd: process.execPath, args: ["-e", script, JSON.stringify(lines), String(status), String(pauseMs)] };
}

test("a failing binary fails the run, and every other binary still runs", async () => {
  const jobs = [
    node("a", ["test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"], 101),
    node("b", ["test result: ok. 3 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.01s"]),
  ];
  const results = await runJobs(jobs, { size: 1, write: () => {} });
  assert.deepEqual(results.map((r) => r.status), [101, 0]);
  const summary = summarise(jobs, results, 1);
  assert.match(summary, /2 jobs in 1\.0s: 4 passed, 1 failed, 2 ignored/);
  assert.match(summary, /failed:\n {2}a \(exit 101\)/);
});

test("a job that cannot start fails the run", async () => {
  const jobs = [{ label: "missing", cmd: join("/no/such/place", "binary"), args: [] }];
  const results = await runJobs(jobs, { size: 2, write: () => {} });
  assert.equal(results[0].status, 1);
  assert.match(results[0].output, /could not start/);
});

test("jobs run side by side, never more than the pool's size, each printed as one block", async () => {
  const jobs = ["a", "b", "c", "d", "e"].map((n) => node(n, [`${n}1`, `${n}2`, `${n}3`], 0, 60));
  const written = [];
  let running = 0;
  let most = 0;
  const run = (...args) => {
    const child = spawn(...args);
    most = Math.max(most, ++running);
    child.on("close", () => running--);
    return child;
  };
  await runJobs(jobs, { size: 2, write: (s) => written.push(s), run });
  // Two at a time while there were jobs left, never three.
  assert.equal(most, 2);
  for (const block of written) {
    const name = block.trim()[0];
    assert.equal(block, `\n     ${name}\n${name}1\n${name}2\n${name}3\n`);
  }
  assert.equal(written.length, 5);
});

test("a job's stdout and stderr keep the order they were written in", async () => {
  // Written one after another with no pause, as cargo writes a heading and then a result.
  const script = `[process.stderr,process.stdout,process.stderr].forEach((w,i)=>w.write("line"+i+"\\n"));`;
  const [result] = await runJobs([{ label: "mixed", cmd: process.execPath, args: ["-e", script] }], { size: 1, write: () => {} });
  assert.equal(result.output, "line0\nline1\nline2\n");
});

// A job that prints `line`, then keeps running until the test opens its gate
// (creates the gate file), so it runs exactly as long as the test needs. A
// test opens the gate in `finally`, so a failing test still lets the child
// end, the run finish and the test process exit.
function gated(line) {
  const dir = mkdtempSync(join(tmpdir(), "test-rust-gate-"));
  const gate = join(dir, "open");
  // A fixed script; the line and the gate's path reach it as arguments
  // (`process.argv[1]` and `[2]` under `node -e`), never as code.
  const script = `const [line, gate] = process.argv.slice(1); console.log(line); const fs = require("fs"); const t = setInterval(() => { if (fs.existsSync(gate)) clearInterval(t); }, 20);`;
  return {
    job: { label: "Running tests/hangs.rs", cmd: process.execPath, args: ["-e", script, line, gate] },
    open: () => writeFileSync(gate, ""),
    remove: () => rmSync(dir, { recursive: true, force: true }),
  };
}

// Waits until `ready()` holds, failing after a generous 30 seconds.
async function until(ready, what) {
  const deadline = Date.now() + 30_000;
  while (!ready()) {
    assert.ok(Date.now() < deadline, `never ${what}`);
    await new Promise((r) => setTimeout(r, 20));
  }
}

test("a binary still running is named, with libtest's slow-test lines, while it runs", async () => {
  const slow = "test hung_test has been running for over 60 seconds";
  const hung = gated(slow);
  const written = [];
  const finished = runJobs([hung.job], { size: 1, write: (s) => written.push(s), every: 50 });
  // The report with the slow line and one more after it, which must not repeat it.
  const reports = () => written.filter((s) => s.includes("still running after"));
  const reportedAgain = () => {
    const at = reports().findIndex((s) => s.includes(slow));
    return at >= 0 && reports().length - at >= 2;
  };
  try {
    await until(reportedAgain, "reported again after the slow line");
  } finally {
    hung.open();
    await finished;
    hung.remove();
  }
  assert.match(reports()[0], /still running after \d+s: Running tests\/hangs\.rs\n/);
  assert.equal(written.join("").split(slow).length - 1, 2, "once in a report, once in the job's block");
  // Before the job's own block, so a log cut off there still has it.
  assert.ok(written.findIndex((s) => s.includes(slow)) < written.length - 1);
});

test("a run that is stopped can report everything each running binary has written", async () => {
  const running = new Set();
  const hung = gated("partial line");
  const finished = runJobs([hung.job], { size: 1, write: () => {}, running });
  let report = "";
  try {
    await until(() => (report = stillRunning(running, true)).includes("partial line"), "reported the partial line");
  } finally {
    hung.open();
    await finished;
    hung.remove();
  }
  assert.match(report, /still running after \d+s: Running tests\/hangs\.rs\npartial line/);
  assert.equal(stillRunning(running, true), "");
});

test("a binary a signal ends fails the run", async () => {
  const jobs = [{ label: "aborts", cmd: process.execPath, args: ["-e", `process.kill(process.pid, "SIGKILL")`] }];
  const [result] = await runJobs(jobs, { size: 1, write: () => {} });
  assert.notEqual(result.status, 0);
  if (process.platform !== "win32") assert.match(result.output, /ended by SIGKILL/);
});

// The whole run, with Node standing in for cargo and for each test binary.
function fakeCargo({ build = 0, binaries }) {
  // A folder that exists, since each binary runs in its package's folder;
  // the executable's name is the key `run` answers by.
  const dir = join(fileURLToPath(import.meta.url), "..");
  const messages = Object.keys(binaries).map((name, i) =>
    artifact("repo", i === 0 ? ["lib"] : ["test"], name, { dir, executable: name }),
  );
  const ran = [];
  const run = (cmd, args, opts) => {
    ran.push([cmd, ...args].join(" "));
    if (cmd === "cargo" && args.includes("--no-run")) {
      // The messages and the exit code reach the fixed script as arguments.
      const script = `JSON.parse(process.argv[1]).forEach((m) => console.log(JSON.stringify(m))); process.exit(Number(process.argv[2]));`;
      return spawn(process.execPath, ["-e", script, JSON.stringify(messages), String(build)], opts);
    }
    const script =
      cmd === "cargo"
        ? `console.log("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s")`
        : binaries[cmd];
    return spawn(process.execPath, ["-e", script], opts);
  };
  return { run, ran };
}

const passes = `console.log("test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s")`;
const fails = `console.log("test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s"); process.exit(101)`;

test("the run exits 0 when every binary and the doctests pass, else 1", async () => {
  const quiet = { write: () => {}, size: 2 };
  const green = fakeCargo({ binaries: { lib: passes, one: passes } });
  assert.equal(await testRust(["--locked"], { ...quiet, run: green.run }), 0);
  assert.ok(green.ran.includes("cargo test --locked --doc"), green.ran.join("\n"));
  assert.ok(green.ran.includes("one") && green.ran.includes("lib"));

  const red = fakeCargo({ binaries: { lib: passes, one: fails, two: passes } });
  assert.equal(await testRust(["--locked"], { ...quiet, run: red.run }), 1);
  assert.ok(red.ran.includes("two"), "the binary after the failing one still ran");
});

test("a failed build ends the run with the build's exit code and runs nothing", async () => {
  const broken = fakeCargo({ build: 101, binaries: { lib: passes } });
  assert.equal(await testRust(["--locked"], { write: () => {}, run: broken.run }), 101);
  assert.equal(broken.ran.length, 1);
});

test("tally adds every test result line", () => {
  const out = [
    "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s",
    "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 3 filtered out; finished in 1.20s",
    // As rustdoc prints it when cargo asks for color.
    "test result: \x1b[32mok\x1b[0m. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s",
  ].join("\n");
  assert.deepEqual(tally(out), { passed: 8, failed: 0, ignored: 1 });
});
