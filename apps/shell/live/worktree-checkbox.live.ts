import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The Worktree checkbox (issue 412; FLOW-NEW-SESSION-R02, R04, R05) in the running app, against a real Core and a real
// git repository: the folder dialog is answered from the test, and the backend is the stand-in.
test("a new session runs in a worktree made from the default branch, and unchecked in the project folder", async ({ ersa }) => {
  const { page } = ersa;
  // A repository whose default branch is main, and a feature branch one commit ahead of it, left checked out in the
  // folder: a new worktree's HEAD is main's tip only if Core made it from the default branch, not what is checked out.
  const repo = path.join(ersa.dir, "repo");
  mkdirSync(repo);
  const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8", windowsHide: true }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.email", "live@example.com");
  git("config", "user.name", "Live check");
  writeFileSync(path.join(repo, "README.md"), "main\n");
  git("add", ".");
  git("commit", "-q", "-m", "On main");
  const mainTip = git("rev-parse", "HEAD");
  git("checkout", "-q", "-b", "feature/rail");
  writeFileSync(path.join(repo, "RAIL.md"), "rail\n");
  git("add", ".");
  git("commit", "-q", "-m", "On the feature branch");

  const screen = page.getByRole("region", { name: "New session" });
  await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(repo);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(screen.getByRole("button", { name: "Project: repo" })).toBeVisible();

  // Checked by default, where the workspace chip stood: after the project, and saying what it means.
  const worktree = screen.getByRole("checkbox", { name: "Worktree" });
  await expect(worktree).toBeChecked();
  await expect(worktree).toHaveAccessibleDescription("The session works in an isolated copy of the repository.");
  await worktree.hover();
  await ersa.shot("worktree-checked");

  // In a short window the project chip's list has no room beneath it, so it opens above it (docs/design/COMPOSER.md,
  // A chip's list).
  await page.setViewportSize({ width: 1000, height: 320 });
  const projectChip = screen.getByRole("button", { name: "Project: repo" });
  await projectChip.click();
  await expect(screen.getByRole("listbox", { name: "Project" })).toBeVisible();
  const opened = await screen.evaluate(() => {
    const list = document.querySelector(".chip-list")!.getBoundingClientRect();
    const from = document.querySelector(".chip[data-open]")!.getBoundingClientRect();
    return { listBottom: list.bottom, listTop: list.top, chipTop: from.top };
  });
  expect(opened.listBottom).toBeLessThanOrEqual(opened.chipTop);
  expect(opened.listTop).toBeGreaterThanOrEqual(0);
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1400, height: 900 });

  // Sent, Core makes the workspace from main: its worktree's HEAD is main's tip, not the feature branch's.
  await screen.getByRole("textbox", { name: "Message" }).fill("Say hello.");
  await screen.getByRole("textbox", { name: "Message" }).press("Enter");
  await expect(screen).toHaveCount(0, { timeout: 30_000 });
  // git lists the repository's own folder first; the one after it is the new workspace.
  const worktrees = git("worktree", "list", "--porcelain")
    .split(/\r?\n\r?\n/)
    .filter((entry) => entry.trim() !== "");
  expect(worktrees).toHaveLength(2);
  expect(/^HEAD (\w+)$/m.exec(worktrees[1]!)?.[1]).toBe(mainTip);

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

  // Under the session, the checkbox shows, unavailable, that it runs in a worktree.
  const controls = page.getByRole("group", { name: "Session controls" });
  await expect(controls.getByRole("checkbox", { name: "Worktree" })).toBeChecked();
  await expect(controls.getByRole("checkbox", { name: "Worktree" })).toBeDisabled();

  // A new session for the project starts checked, as the last one was; unchecked, it runs in the project folder and
  // no worktree is made.
  await page.getByRole("navigation", { name: "Rail" }).getByRole("button", { name: "New session in repo" }).click();
  const again = page.getByRole("region", { name: "New session" }).getByRole("checkbox", { name: "Worktree" });
  await expect(again).toBeChecked();
  await again.uncheck();
  await ersa.shot("worktree-unchecked");
  await page.getByRole("textbox", { name: "Message" }).fill("Say hello again.");
  await page.getByRole("textbox", { name: "Message" }).press("Enter");
  await expect(page.getByRole("region", { name: "New session" })).toHaveCount(0, { timeout: 30_000 });
  await expect(page.getByRole("group", { name: "Session controls" }).getByRole("checkbox", { name: "Worktree" })).not.toBeChecked();
  expect(git("worktree", "list", "--porcelain").split(/\r?\n\r?\n/).filter((entry) => entry.trim() !== "")).toHaveLength(2);

  // And the next new session starts unchecked, as the user last left it (FLOW-NEW-SESSION-R02).
  await page.getByRole("navigation", { name: "Rail" }).getByRole("button", { name: "New session in repo" }).click();
  await expect(page.getByRole("region", { name: "New session" }).getByRole("checkbox", { name: "Worktree" })).not.toBeChecked();
  await ersa.shot("new-session-again");
});
