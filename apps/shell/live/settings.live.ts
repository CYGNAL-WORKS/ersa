import type { Page } from "@playwright/test";
import { expect, test } from "./harness";

// The settings page in the running window (#544; LAY-SHELL, The settings page; FLOW-CLEAN-UP-R15): opened from
// Settings in the arrange menu, Archive inactive sessions changed, and the value kept when the page opens again.

const rail = (page: Page) => page.getByRole("navigation", { name: "Rail" });
const chip = (page: Page) => page.getByRole("button", { name: /^Archive inactive sessions:/ });

async function openSettings(page: Page) {
  const arrange = rail(page).getByRole("button", { name: "Arrange sessions" });
  await expect(arrange).toBeEnabled({ timeout: 60_000 });
  await arrange.click();
  await page.getByRole("menuitem", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Settings" })).toBeVisible();
}

test("Archive inactive sessions is changed on the settings page and kept", async ({ ersa }) => {
  test.setTimeout(180_000);
  const page = ersa.page;
  await openSettings(page);
  await expect(page.getByRole("navigation", { name: "Settings sections" }).getByRole("button", { name: "Storage" })).toHaveAttribute("aria-current", "true");
  await expect(chip(page)).toHaveAccessibleName("Archive inactive sessions: Never");
  await ersa.shot("opened");

  await chip(page).click();
  await page.getByRole("option", { name: "7 days" }).click();
  await expect(chip(page)).toHaveAccessibleName("Archive inactive sessions: 7 days");
  // Core has it once the chip is no longer busy.
  await expect(chip(page)).not.toHaveAttribute("aria-busy", "true");
  await ersa.shot("changed");

  // Left for New session and opened again, it shows the value kept.
  await rail(page).getByRole("button", { name: "New session" }).first().click();
  await expect(page.getByRole("heading", { level: 1, name: "Settings" })).toHaveCount(0);
  await openSettings(page);
  await expect(chip(page)).toHaveAccessibleName("Archive inactive sessions: 7 days");

  await ersa.shot("reopened");

  // Never clears it.
  await chip(page).click();
  await page.getByRole("option", { name: "Never" }).click();
  await expect(chip(page)).toHaveAccessibleName("Archive inactive sessions: Never");
  await expect(chip(page)).not.toHaveAttribute("aria-busy", "true");
  await ersa.shot("never");
});
