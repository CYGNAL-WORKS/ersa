// Tests for test-rust.mjs, run without cargo: Node stands in for the test binaries.
// Run: node --test scripts/test-rust.test.mjs

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { planJobs, runJobs, splitArgs, stillRunning, summarise, tally, testRust } from "./test-rust.mjs";

// A compiler-artifact message as `cargo test --no-run --message-format=json` prints it.
function artifact(pkg, kind, name, { test = true, src = `src/${name}.rs` } = {}) {
  const dir = join("/repo/crates", pkg);
  return {
    reason: "compiler-artifact",
    package_id: `path+file:///repo/crates/${pkg}#0.1.0`,
    manifest_path: join(dir, "Cargo.toml"),
    target: { kind, name, src_path: join(dir, src) },
    profile: { test },
    executable: test || kind.includes("bin") ? join("/repo/target", `${name}-${pkg}`) : null,
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
function node(label, lines, status = 0, pauseMs = 30) {
  const script = `const l=${JSON.stringify(lines)};let i=0;const t=setInterval(()=>{if(i<l.length){process.stdout.write(l[i++]+"\\n")}else{clearInterval(t);process.exit(${status})}},${pauseMs});`;
  return { label, cmd: process.execPath, args: ["-e", script] };
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
  const { spawn } = await import("node:child_process");
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

test("a binary still running is named, with libtest's slow-test lines, while it runs", async () => {
  const slow = "test hung_test has been running for over 60 seconds";
  const jobs = [
    { label: "Running tests/hangs.rs", cmd: process.execPath, args: ["-e", `console.log(${JSON.stringify(slow)}); setTimeout(() => {}, 600);`] },
  ];
  const written = [];
  await runJobs(jobs, { size: 1, write: (s) => written.push(s), every: 150 });
  const reports = written.filter((s) => s.includes("still running after"));
  assert.ok(reports.length >= 2, written.join(""));
  assert.match(reports[0], /still running after \d+s: Running tests\/hangs\.rs\n/);
  // The slow-test line once, in the first report after it was written.
  assert.equal(written.join("").split(slow).length - 1, 2, "once in a report, once in the job's block");
  // Before the job's own block, so a log cut off there still has it.
  assert.ok(written.findIndex((s) => s.includes(slow)) < written.length - 1);
});

test("a run that is stopped can report everything each running binary has written", async () => {
  const running = new Map();
  const jobs = [{ label: "Running tests/hangs.rs", cmd: process.execPath, args: ["-e", `console.log("partial line"); setTimeout(() => {}, 800);`] }];
  const finished = runJobs(jobs, { size: 1, write: () => {}, running });
  let report = "";
  while (!report.includes("partial line")) {
    await new Promise((r) => setTimeout(r, 50));
    report = stillRunning(running, true);
  }
  assert.match(report, /still running after \d+s: Running tests\/hangs\.rs\npartial line/);
  await finished;
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
  const manifest = join(fileURLToPath(import.meta.url), "..", "Cargo.toml");
  const message = (name, kind) => ({
    reason: "compiler-artifact",
    package_id: "path+file:///repo#0.1.0",
    manifest_path: manifest,
    target: { kind, name, src_path: join(manifest, "..", `${name}.rs`) },
    profile: { test: true },
    executable: name,
  });
  const messages = Object.keys(binaries).map((name, i) => message(name, i === 0 ? ["lib"] : ["test"]));
  const ran = [];
  const run = (cmd, args, opts) => {
    ran.push([cmd, ...args].join(" "));
    let script;
    if (cmd === "cargo" && args.includes("--no-run")) {
      script = `${JSON.stringify(messages)}.forEach((m) => console.log(JSON.stringify(m))); process.exit(${build});`;
    } else if (cmd === "cargo") {
      script = `console.log("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s")`;
    } else {
      script = binaries[cmd];
    }
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
