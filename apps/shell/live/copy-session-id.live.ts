import { mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Copy session ID and a session whose folder is gone, in the running window (#340, #524; FLOW-RESUME-SESSION-R16, R18,
// R19, R03, PROTO-CORE-API-R80), with the stand-in backend: the tile's menu copies the id Core lists once Core has found
// the session's record, and writing to a stopped session whose folder was deleted says so in the catalog's words and
// keeps the session.
test("Copy session ID copies the listed id, and writing to a session whose folder is gone says so and keeps it", async ({ ersa }) => {
  test.setTimeout(300_000);
  const folder = path.join(ersa.dir, "copy-id-check");
  mkdirSync(folder);

  let page = ersa.page;
  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  let rail = page.getByRole("navigation", { name: "Rail" });
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Hello there");
  await words.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 120_000 });

  // The stand-in, like the CLI, writes the session's record with its first message, but ends no turn: a reopened
  // window's list is where Core looks for the record (FLOW-RESUME-SESSION-R19).
  await ersa.restart();
  page = ersa.page;
  rail = page.getByRole("navigation", { name: "Rail" });

  // A live check never touches the owner's clipboard or raises its permission prompt, so the page's clipboard write
  // is replaced by a recorder; the App test holds the exact-id rule with its own stand-in.
  await page.evaluate(() => {
    const written: string[] = [];
    (window as unknown as { copied: string[] }).copied = written;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (data: string) => void written.push(data) },
    });
  });
  await rail.locator(".session").first().hover();
  // The stand-in backend gives the session no title. More sits beside the tile's button, not in it.
  await rail.getByRole("button", { name: "More for Untitled session" }).click();
  const item = page.getByRole("menuitem", { name: /^Copy session ID/ });
  await expect(item).toBeVisible();
  await expect(item).not.toHaveAttribute("aria-disabled", "true", { timeout: 30_000 });
  await ersa.shot("menu-copy-session-id");
  await item.click();
  const copied = await page.evaluate(() => (window as unknown as { copied: string[] }).copied);
  expect(copied).toHaveLength(1);
  // The backend's id, never Ersa's own ("s-…"); the App test holds the exact id.
  expect(copied[0]).toMatch(/\S/);
  expect(copied[0]).not.toMatch(/^s-/);

  // Quit, which stops the session's backend, delete its folder, and write to it after reopening.
  await ersa.restart();
  rmSync(folder, { recursive: true, force: true });
  page = ersa.page;
  rail = page.getByRole("navigation", { name: "Rail" });
  await rail.locator(".session").first().click();
  const again = page.getByRole("textbox", { name: "Message" });
  await again.fill("Are you still there?");
  await again.press("Enter");
  await expect(page.locator(".composer-status-line")).toHaveText("This session's folder no longer exists. Put the folder back, then try again.", { timeout: 60_000 });
  // The session stays where it was. The stand-in backend keeps no record, so its history is not reread here.
  await expect(rail.locator(".session")).toHaveCount(1);
  await ersa.shot("folder-gone-light");

  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForTimeout(300);
  await ersa.shot("folder-gone-dark");
  await page.emulateMedia({ colorScheme: "light" });
});
