import { mkdirSync } from "node:fs";
import path from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "./harness";

// Mark as unread in the running window, with the stand-in backend (#324; FLOW-ORGANIZE-SESSIONS-R24, R25): offered on
// a session that isn't the active one and unavailable with its reason on the active one, and the mark kept by Core
// through a restart of Ersa and of Core, until the session is opened.

const rail = (page: Page) => page.getByRole("navigation", { name: "Rail" });

async function newSession(page: Page, text: string) {
  await rail(page).getByRole("button", { name: /^New session in / }).click();
  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill(text);
  await words.press("Enter");
}

test("Mark as unread keeps a session unseen across a restart until it is opened", async ({ ersa }) => {
  test.setTimeout(240_000);
  const folder = path.join(ersa.dir, "unseen-check");
  mkdirSync(folder);
  let page = ersa.page;
  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail(page).getByRole("button", { name: /^New session in / })).toBeVisible();

  await newSession(page, "first");
  await expect(rail(page).locator("[data-session-id]")).toHaveCount(1, { timeout: 60_000 });
  await newSession(page, "second");
  await expect(rail(page).locator("[data-session-id]")).toHaveCount(2, { timeout: 60_000 });

  // The newest is active: Mark as unread is unavailable on it, with the reason.
  const active = rail(page).locator(".session[aria-current]");
  const activeId = await rail(page).locator("[data-session-id]").filter({ has: page.locator(".session[aria-current]") }).getAttribute("data-session-id");
  await active.click({ button: "right" });
  const unavailable = page.getByRole("menuitem", { name: /^Mark as unread/ });
  await expect(unavailable).toHaveAttribute("aria-disabled", "true");
  await expect(unavailable).toContainText("Available once you open another session");
  await ersa.shot("active-unavailable");
  await page.keyboard.press("Escape");

  // The other one is marked unread: its title reads strong.
  const otherId = await rail(page).locator("[data-session-id]").filter({ has: page.locator(".session:not([aria-current])") }).getAttribute("data-session-id");
  const otherTile = () => rail(page).locator(`[data-session-id="${otherId}"] .session`);
  await otherTile().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Mark as unread", exact: true }).click();
  await expect(otherTile().locator(".session-title")).toHaveAttribute("data-unseen", "true");
  await ersa.shot("marked-unread");

  // Quit and open again, Core too: Core kept the mark. The window opens on the newest session, as it does at every
  // start, and only then is the mark read: until a session is open, the one marked could still be opened, and seen.
  await ersa.restart();
  page = ersa.page;
  const activeTile = () => rail(page).locator(`[data-session-id="${activeId}"] .session`);
  await expect(activeTile()).toHaveAttribute("aria-current", "true", { timeout: 60_000 });
  await expect(otherTile().locator(".session-title")).toHaveAttribute("data-unseen", "true");
  await ersa.shot("unseen-after-restart");

  // Opened, it is seen, and stays seen through another restart.
  await otherTile().click();
  await expect(otherTile()).toHaveAttribute("aria-current", "true");
  // Back on the other session, the tile reads as Core now has it: seen.
  await rail(page).locator(`[data-session-id="${activeId}"] .session`).click();
  await expect(otherTile()).not.toHaveAttribute("aria-current", "true");
  await expect(otherTile().locator(".session-title")).not.toHaveAttribute("data-unseen");
  await ersa.restart();
  page = ersa.page;
  await expect(otherTile()).toBeVisible({ timeout: 60_000 });
  await expect(otherTile().locator(".session-title")).not.toHaveAttribute("data-unseen");
  await ersa.shot("seen-after-restart");
});
