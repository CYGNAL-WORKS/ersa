import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import type { Page } from "@playwright/test";
import { type Ersa, expect, test } from "./harness";

// The Clean up list in the running window (#545; FLOW-CLEAN-UP, steps 5 to 7; R16 to R18), against a real Core, a
// real git repository and the stand-in backend. A leftover workspace can't be made here: Core counts a workspace as one
// only an hour after it was made, and has no clock a live check can move, so the window's tests over a fake Core cover
// it.

/** A project in a new repository with one commit on main, and a session started in a worktree of it. */
async function projectWithSession(ersa: Ersa) {
  const repo = path.join(ersa.dir, "repo");
  mkdirSync(repo);
  const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8", windowsHide: true }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.email", "live@example.com");
  git("config", "user.name", "Live check");
  writeFileSync(path.join(repo, "README.md"), "main\n");
  git("add", ".");
  git("commit", "-q", "-m", "On main");

  const page = ersa.page;
  const screen = page.getByRole("region", { name: "New session" });
  await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(repo);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(screen.getByRole("checkbox", { name: "Worktree" })).toBeChecked();
  await screen.getByRole("textbox", { name: "Message" }).fill("Say hello.");
  await screen.getByRole("textbox", { name: "Message" }).press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 120_000 });

  // The session's worktree: git lists the repository's own folder first.
  const entry = git("worktree", "list", "--porcelain").split(/\r?\n\r?\n/).filter((e) => e.trim() !== "")[1]!;
  const folder = /^worktree (.+)$/m.exec(entry)![1]!;
  return { git, folder };
}

async function openStorage(page: Page) {
  const arrange = page.getByRole("navigation", { name: "Rail" }).getByRole("button", { name: "Arrange sessions" });
  await expect(arrange).toBeEnabled({ timeout: 60_000 });
  await arrange.click();
  await page.getByRole("menuitem", { name: "Settings" }).click();
  const list = page.getByRole("region", { name: "Clean up list" });
  await expect(list).toBeVisible();
  return list;
}

test("the list shows an unused branch ticked and a stray folder unticked, and one approval removes the ticked one", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { git, folder } = await projectWithSession(ersa);
  // A branch named as Ersa names its own, that no session uses, whose changes main already holds.
  git("branch", "ersa/left-behind", "main");
  // A folder beside the session's, in the project's folder under the worktrees root, that no workspace names.
  const stray = path.join(path.dirname(folder), "stray-folder");
  mkdirSync(stray);
  writeFileSync(path.join(stray, "notes.txt"), "something\n");

  const page = ersa.page;
  const list = await openStorage(page);
  const branch = list.getByRole("checkbox", { name: "ersa/left-behind" });
  const strayBox = list.getByRole("checkbox", { name: "stray-folder" });
  await expect(branch).toBeChecked({ timeout: 120_000 });
  await expect(branch).toBeEnabled();
  await expect(list.getByText("Its changes are already in the base branch.")).toBeVisible();
  await expect(strayBox).toBeEnabled({ timeout: 120_000 });
  await expect(strayBox).not.toBeChecked();
  // A running session's folder is not on the list.
  await expect(list.getByRole("heading", { name: "Inactive and kept sessions" })).toHaveCount(0);
  await ersa.shot("listed");

  await list.getByRole("button", { name: "Clean up…" }).click();
  const question = page.getByRole("dialog");
  await expect(question.getByRole("heading")).toHaveText("Clean up 1 branch?");
  await ersa.shot("asked");
  // Nothing is removed before the question is confirmed.
  expect(git("branch", "--list", "ersa/left-behind")).not.toBe("");
  await question.getByRole("button", { name: "Clean up", exact: true }).click();

  await expect(question).toHaveCount(0, { timeout: 120_000 });
  await expect(list.getByRole("status").filter({ hasText: "Removed 1 of 1 item." })).toBeVisible();
  expect(git("branch", "--list", "ersa/left-behind")).toBe("");
  expect(existsSync(stray)).toBe(true);
  // The list is read again: the branch is gone and the folder is still offered.
  await expect(list.getByRole("checkbox", { name: "ersa/left-behind" })).toHaveCount(0);
  await expect(list.getByRole("checkbox", { name: "stray-folder" })).toBeVisible({ timeout: 120_000 });
  await ersa.shot("cleaned-up");
});

test("a folder kept after archiving because it holds a changed file is listed unticked, with what it loses", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { folder } = await projectWithSession(ersa);
  writeFileSync(path.join(folder, "unsaved.txt"), "work in progress\n");

  // The stand-in ends no turn, so the session is stopped by quitting Ersa, as Archive needs.
  await ersa.restart();
  const page = ersa.page;
  const rail = page.getByRole("navigation", { name: "Rail" });
  const tile = rail.locator(".session").first();
  await expect(tile).toBeVisible({ timeout: 60_000 });
  await tile.hover();
  await rail.getByRole("button", { name: "More for Untitled session" }).click();
  await page.getByRole("menuitem", { name: "Archive" }).click();

  const list = await openStorage(page);
  await expect(list.getByRole("heading", { name: "Inactive and kept sessions" })).toBeVisible({ timeout: 120_000 });
  const kept = list.getByRole("checkbox", { name: "Untitled session" });
  await expect(kept).toBeEnabled({ timeout: 120_000 });
  await expect(kept).not.toBeChecked();
  await expect(list.getByText("Removing it loses 1 changed file for good.")).toBeVisible();
  expect(existsSync(folder)).toBe(true);
  await ersa.shot("kept-listed");
});
