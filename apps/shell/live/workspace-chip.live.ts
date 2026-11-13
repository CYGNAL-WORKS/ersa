import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The workspace chip's base branch (#184; FLOW-NEW-SESSION-R21, R29) in the running app, against a real Core and a
// real git repository: the folder dialog is answered from the test, and the backend is the stand-in.
test("a new workspace starts at the default branch, and is made from the base branch chosen", async ({ ersa }) => {
  const { page } = ersa;
  // A repository whose default branch is main, with a release branch one commit ahead of it.
  const repo = path.join(ersa.dir, "repo");
  mkdirSync(repo);
  const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8", windowsHide: true }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.email", "live@example.com");
  git("config", "user.name", "Live check");
  writeFileSync(path.join(repo, "README.md"), "main\n");
  git("add", ".");
  git("commit", "-q", "-m", "On main");
  git("checkout", "-q", "-b", "release/1.0");
  writeFileSync(path.join(repo, "RELEASE.md"), "release\n");
  git("add", ".");
  git("commit", "-q", "-m", "On release");
  const releaseTip = git("rev-parse", "HEAD");
  // The folder is left on a feature branch at main's commit: the chip starts at the default branch, not at what the
  // folder has checked out, and a new workspace's HEAD can be release's tip only if it was made from release.
  git("checkout", "-q", "-b", "feature/rail", "main");

  const screen = page.getByRole("region", { name: "New session" });
  await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(repo);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(screen.getByRole("button", { name: "Project: repo" })).toBeVisible();

  // R21, R29: the chip names a new workspace from the default branch, not the one the folder has checked out.
  const chip = screen.getByRole("button", { name: /^Workspace:/ });
  await expect(chip).toHaveAccessibleName("Workspace: New workspace from main");

  // In a short window the list has no room beneath its chip, so it opens above it (docs/design/COMPOSER.md, A chip's
  // list), then beneath again where there is room.

  await page.setViewportSize({ width: 1000, height: 420 });
  await chip.click();
  await expect(screen.getByRole("listbox", { name: "Workspace" })).toBeVisible();
  const opened = await screen.evaluate(() => {
    const list = document.querySelector(".chip-list")!.getBoundingClientRect();
    const from = document.querySelector(".chip[data-open]")!.getBoundingClientRect();
    return { listBottom: list.bottom, listTop: list.top, chipTop: from.top };
  });
  expect(opened.listBottom).toBeLessThanOrEqual(opened.chipTop);
  expect(opened.listTop).toBeGreaterThanOrEqual(0);
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1400, height: 900 });
  await chip.click();
  const list = screen.getByRole("listbox", { name: "Workspace" });
  const search = list.getByRole("searchbox", { name: "Search branches" });
  await expect(search).toBeFocused();
  const base = list.getByRole("group", { name: "Base branch" });
  await expect(base.getByRole("option")).toHaveText(["feature/rail", /^main\s*Default$/, "release/1.0"]);
  await ersa.shot("workspace-list");
  await search.fill("rel");
  await expect(base.getByRole("option")).toHaveText(["release/1.0"]);
  await search.press("Enter");
  await expect(chip).toHaveAccessibleName("Workspace: New workspace from release/1.0");
  await expect(chip).toBeFocused();

  // Sent, Core makes the workspace from release/1.0: its worktree's HEAD is the release branch's tip.
  await screen.getByRole("textbox", { name: "Message" }).fill("Say hello.");
  await screen.getByRole("textbox", { name: "Message" }).press("Enter");
  await expect(screen).toHaveCount(0, { timeout: 30_000 });
  // git lists the repository's own folder first; the one after it is the new workspace.
  const worktrees = git("worktree", "list", "--porcelain")
    .split(/\r?\n\r?\n/)
    .filter((entry) => entry.trim() !== "");
  expect(worktrees).toHaveLength(2);
  expect(/^HEAD (\w+)$/m.exec(worktrees[1]!)?.[1]).toBe(releaseTip);

  // Under the session, in a wide window, the session controls sit in the composer's column at the measure,
  // space-xs beneath the box, with room at the window's foot (docs/design/COMPOSER.md, The composer).
  await page.setViewportSize({ width: 1600, height: 900 });
  const composer = page.locator(".composer");
  await expect(composer.locator(".session-controls")).toBeVisible();
  const layout = await composer.evaluate((column) => {
    const tokens = getComputedStyle(column);
    const px = (name: string) => {
      const probe = column.appendChild(document.createElement("div"));
      probe.style.width = `var(${name})`;
      const width = probe.getBoundingClientRect().width;
      probe.remove();
      return width;
    };
    const box = column.querySelector(".composer-box")!.getBoundingClientRect();
    const controls = column.querySelector(".session-controls")!.getBoundingClientRect();
    return { box, controls, xs: px("--space-xs"), lg: px("--space-lg"), measure: px("--size-measure"), padding: tokens.paddingBottom, window: innerHeight };
  });
  expect(layout.box.width).toBeLessThanOrEqual(layout.measure);
  expect(layout.controls.left).toBeGreaterThanOrEqual(layout.box.left - 0.5);
  expect(layout.controls.right).toBeLessThanOrEqual(layout.box.right + 0.5);
  expect(layout.controls.top - layout.box.bottom).toBeCloseTo(layout.xs, 0);
  expect(layout.window - layout.controls.bottom).toBeGreaterThanOrEqual(layout.lg - 0.5);
  await ersa.shot("session-wide");

  // A new session starts at the default branch again: the base chosen for the last one is not remembered.
  await page.getByRole("navigation", { name: "Rail" }).getByRole("button", { name: "New session in repo" }).click();
  await expect(page.getByRole("button", { name: /^Workspace:/ })).toHaveAccessibleName("Workspace: New workspace from main");
  await ersa.shot("new-session-again");
});
