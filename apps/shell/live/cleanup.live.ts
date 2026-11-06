import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { expect, test } from "./harness";

// The harness leaves no Core, no window and no data folder behind, even when a live check fails (#207). This check
// runs a live check that fails on purpose in a Playwright of its own, then looks for what that run started.

const FAIL_ON_PURPOSE = "ERSA_LIVE_FAIL_ON_PURPOSE";

test("a failing live check leaves no process or folder behind", async () => {
  test.skip(!!process.env[FAIL_ON_PURPOSE], "this is the run that fails on purpose");
  // The second run builds (quickly, the build is current) and starts a window of its own.
  test.setTimeout(5 * 60_000);
  // Outside this run's results folder, which a second Playwright would empty as it starts.
  const elsewhere = mkdtempSync(path.join(tmpdir(), "ersa-live-cleanup-"));
  let label: string;
  let dir: string;
  try {
    const record = path.join(elsewhere, "failed-run.json");
    let failed = false;
    try {
      // Playwright's own CLI under this Node, with no shell between: the timeout below stops the runner itself, and
      // its worker, losing the runner, exits and ends what it started (harness.ts).
      const shell = path.resolve(import.meta.dirname, "..");
      const cli = path.join(shell, "node_modules", "@playwright", "test", "cli.js");
      const args = [cli, "test", "-c", "playwright.live.config.ts", "live/cleanup.live.ts", "-g", "fails on purpose"];
      execFileSync(process.execPath, [...args, "--output", path.join(elsewhere, "results")], {
        cwd: shell,
        env: { ...process.env, [FAIL_ON_PURPOSE]: record },
        stdio: "pipe",
        timeout: 4 * 60_000,
      });
    } catch {
      failed = true;
    }
    expect(failed, "the run that fails on purpose failed").toBe(true);
    ({ label, dir } = JSON.parse(readFileSync(record, "utf8")) as { label: string; dir: string });
  } finally {
    rmSync(elsewhere, { recursive: true, force: true });
  }
  // Core's endpoint and the window's own endpoint are both named after the label; neither is served any more.
  expect(readdirSync("\\\\.\\pipe\\").filter((name) => name.includes(label))).toEqual([]);
  expect(existsSync(dir), `${dir} was removed`).toBe(false);
});

test.describe(() => {
  // Skipped before the harness starts anything, except in the run the check above starts.
  test.skip(!process.env[FAIL_ON_PURPOSE], "run only by the check above");

  test("fails on purpose", async ({ ersa }) => {
    writeFileSync(process.env[FAIL_ON_PURPOSE]!, JSON.stringify({ label: ersa.label, dir: ersa.dir }));
    expect(ersa.label, "failing on purpose").toBe("something else");
  });
});
