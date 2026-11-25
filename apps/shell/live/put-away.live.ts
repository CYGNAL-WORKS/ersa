import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Archiving frees the folder and resuming brings it back, in the running window (#539; FLOW-CLEAN-UP-R22, R06, R03,
// R21), against a real Core, a real git repository and the stand-in backend: a session alone in a clean worktree is
// archived, the sweep the archive asks for removes its folder and keeps its branch, and the session, shown with the
// archived ones, is resumed from its note and written to, its folder back at the same path.
test("archiving a session puts its folder away, and resuming it puts the folder back", async ({ ersa }) => {
  test.setTimeout(300_000);
  const repo = path.join(ersa.dir, "repo");
  mkdirSync(repo);
  const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { encoding: "utf8", windowsHide: true }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.email", "live@example.com");
  git("config", "user.name", "Live check");
  writeFileSync(path.join(repo, "README.md"), "main\n");
  git("add", ".");
  git("commit", "-q", "-m", "On main");

  let page = ersa.page;
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
  const worktrees = () =>
    git("worktree", "list", "--porcelain")
      .split(/\r?\n\r?\n/)
      .filter((entry) => entry.trim() !== "");
  const entry = worktrees()[1]!;
  const folder = /^worktree (.+)$/m.exec(entry)![1]!;
  const branch = /^branch refs\/heads\/(.+)$/m.exec(entry)![1]!;
  expect(existsSync(path.join(folder, "README.md"))).toBe(true);

  // The stand-in ends no turn, so the session is stopped by quitting Ersa, as Archive needs.
  await ersa.restart();
  page = ersa.page;
  const rail = page.getByRole("navigation", { name: "Rail" });
  const tile = rail.locator(".session").first();
  await expect(tile).toBeVisible({ timeout: 60_000 });
  await tile.hover();
  // The stand-in backend gives the session no title. More sits beside the tile's button, not in it.
  await rail.getByRole("button", { name: "More for Untitled session" }).click();
  await page.getByRole("menuitem", { name: "Archive" }).click();

  // The sweep the archive asked for removes the folder and git's record, and keeps the branch.
  await expect.poll(() => existsSync(folder), { timeout: 120_000 }).toBe(false);
  expect(worktrees()).toHaveLength(1);
  expect(git("branch", "--list", branch)).not.toBe("");
  await ersa.shot("archived-folder-gone");

  // Shown with the archived sessions, it opens read-only on its note.
  await rail.getByRole("button", { name: "Arrange sessions" }).click();
  await page.getByRole("menuitem", { name: /^Show/ }).click();
  await page.getByRole("menuitemradio", { name: "Archived" }).click();
  await page.keyboard.press("Escape");
  const archived = rail.locator(".session").first();
  await expect(archived).toBeVisible();
  await archived.click();
  await expect(page.getByText("This session is archived. Resume it to send a message.")).toBeVisible();
  await ersa.shot("archived-note");

  // Resumed from its note, its folder is back at the same path, checked out on its branch.
  await page.getByRole("button", { name: "Resume", exact: true }).click();
  const words = page.getByRole("textbox", { name: "Message" });
  await expect(words).toBeVisible({ timeout: 120_000 });
  expect(existsSync(path.join(folder, "README.md"))).toBe(true);
  expect(execFileSync("git", ["-C", folder, "branch", "--show-current"], { encoding: "utf8", windowsHide: true }).trim()).toBe(branch);

  // And it runs: a message is taken.
  await words.fill("Say it again.");
  await words.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" }).getByText("Say it again.")).toBeVisible({ timeout: 60_000 });
  await ersa.shot("resumed-and-written-to");
});
