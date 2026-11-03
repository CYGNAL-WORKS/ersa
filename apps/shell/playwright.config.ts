import { defineConfig } from "@playwright/test";

// Screenshot tests against the gallery (ADR 0008). Baselines are rendered on
// Linux in the Playwright container, and CI's runner has the same OS, Chromium,
// and fonts, so fonts and anti-aliasing match; regenerate them with
// `pnpm test:screenshots:update` (Docker), never on a developer's own OS.
export default defineConfig({
  testDir: "e2e",
  // One baseline per test, with no platform suffix: only Linux renders them.
  snapshotPathTemplate: "{testDir}/__screenshots__/{arg}{ext}",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: "http://localhost:5174",
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 1,
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
  webServer: {
    command: "pnpm exec vite --port 5174 --strictPort",
    url: "http://localhost:5174/gallery.html",
    reuseExistingServer: !process.env.CI,
  },
});
