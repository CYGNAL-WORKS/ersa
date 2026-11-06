import { defineConfig } from "@playwright/test";

// Live checks: Playwright drives the real Ersa window on Windows, attached to
// a Core of its own (docs/agents/live-checks.md). Run one with
// `pnpm live live/<name>.live.ts`. They are local only, never part of CI.
export default defineConfig({
  testDir: "live",
  testMatch: "**/*.live.ts",
  globalSetup: "./live/build.ts",
  // One window at a time: each test starts its own Core and window, and windows side by side steal focus.
  workers: 1,
  timeout: 120_000,
  forbidOnly: true,
  reporter: [["list"], ["html", { open: "never", outputFolder: "live-report" }]],
  // Screenshots and the processes' logs, per test; the pull request's screenshots come from here.
  outputDir: "live-results",
});
