// Playwright's global setup for the live checks: builds what they run, from
// this checkout, before any check starts. Cargo and Vite rebuild only what
// changed, so a second run starts in seconds.

import { execFileSync } from "node:child_process";
import path from "node:path";
import { root, targetDir } from "./paths";

export default function build() {
  if (process.platform !== "win32") {
    throw new Error("live checks run on Windows: they drive the window through WebView2");
  }
  const run = (program: string, args: string[], cwd: string) =>
    // `shell` so Windows finds pnpm's and cargo's .cmd and .exe shims on PATH.
    execFileSync(program, args, { cwd, stdio: "inherit", shell: true });
  // The Interface the window embeds.
  run("pnpm", ["build"], path.join(root, "apps", "shell"));
  // Core, the stand-in backend, and the window with the Interface embedded (Tauri's custom-protocol feature).
  run(
    "cargo",
    ["build", "-p", "ersa-cli", "-p", "ersa-shell", "--bins", "--examples", "--features", "tauri/custom-protocol", "--target-dir", targetDir],
    root,
  );
}
