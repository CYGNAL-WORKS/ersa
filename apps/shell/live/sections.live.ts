import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Sections in the running window, on the stand-in backend (FLOW-ORGANIZE-SESSIONS, steps 3 and 5): a tile's Move to
// section ▸ New section… groups the rail by section and makes the section with the session in it; the arrange menu's
// New section makes an empty one; Move to section moves the session between them. Every section shows, empty or not,
// in the order made, then No section; a heading's Rename works in place; Delete asks nothing and keeps every session,
// under No section.
test("sections made from the window, a heading renames in place and deletes keeping its sessions", async ({ ersa }) => {
  test.setTimeout(180_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "sections-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });
  // Each shot waits for the rail's motion to finish, leaving out the working glyph's, which loops.
  const settled = () =>
    page.waitForFunction(() => document.getAnimations().every((a) => a.playState !== "running" || a.effect?.getComputedTiming().iterations === Infinity));

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();
  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Hello");
  await words.press("Enter");
  const tile = rail.locator("[data-session-id]").first();
  await expect(tile).toBeVisible({ timeout: 60_000 });
  const id = await tile.getAttribute("data-session-id");
  const names = rail.locator(".rail-projects > .rail-project .rail-heading-name");
  const nameField = rail.getByRole("textbox", { name: "Section name" });
  const moveTo = async (item: string) => {
    await tile.hover();
    await tile.getByRole("button", { name: /^More for / }).click();
    await page.getByRole("menuitem", { name: "Move to section" }).click();
    await page.getByRole(item === "New section…" ? "menuitem" : "menuitemradio", { name: item }).click();
  };

  // Grouped by project, a tile's New section… groups by section and shows the new heading's empty field.
  await moveTo("New section…");
  await expect(nameField).toBeFocused();
  await expect(nameField).toHaveValue("");
  await settled();
  await ersa.shot("new-section-field");
  await nameField.fill("Docs");
  await nameField.press("Enter");
  await expect(names).toHaveText(["Docs"]);
  await expect(rail.locator(".rail-projects > .rail-project", { hasText: "Docs" }).locator(`[data-session-id="${id}"]`)).toBeVisible();

  // The arrange menu's New section makes an empty one, after the others.
  await rail.getByRole("button", { name: "Arrange sessions" }).click();
  await page.getByRole("menuitem", { name: "New section" }).click();
  await expect(nameField).toBeFocused();
  await nameField.fill("Later");
  await nameField.press("Enter");
  await expect(names).toHaveText(["Docs", "Later"]);
  await settled();
  await ersa.shot("grouped-by-section");

  // Move to section shows where the session is, and moves it.
  await tile.hover();
  await tile.getByRole("button", { name: /^More for / }).click();
  await page.getByRole("menuitem", { name: "Move to section" }).click();
  await expect(page.getByRole("menuitemradio", { name: "Docs" })).toHaveAttribute("aria-checked", "true");
  await settled();
  await ersa.shot("move-to-section");
  await page.getByRole("menuitemradio", { name: "Later" }).click();
  await expect(rail.locator(".rail-projects > .rail-project", { hasText: "Later" }).locator(`[data-session-id="${id}"]`)).toBeVisible();
  await moveTo("Docs");
  await expect(rail.locator(".rail-projects > .rail-project", { hasText: "Docs" }).locator(`[data-session-id="${id}"]`)).toBeVisible();

  // Rename, in place: the heading's name becomes a field; Enter saves the name Core keeps.
  const later = rail.locator(".rail-heading", { hasText: "Later" });
  await later.hover();
  await later.getByRole("button", { name: "More for Later" }).click();
  await page.getByRole("menuitem", { name: "Rename" }).click();
  const field = rail.getByRole("textbox", { name: "Section name" });
  await expect(field).toBeFocused();
  await settled();
  await ersa.shot("renaming");
  await field.fill("Someday");
  await field.press("Enter");
  await expect(names).toHaveText(["Docs", "Someday"]);

  // Delete asks nothing, and the session goes to No section.
  const heading = rail.locator(".rail-heading", { hasText: "Docs" });
  await heading.hover();
  await heading.getByRole("button", { name: "More for Docs" }).click();
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await expect(names).toHaveText(["Someday", "No section"]);
  await expect(rail.locator(".rail-projects > .rail-project", { hasText: "No section" }).locator(`[data-session-id="${id}"]`)).toBeVisible();
  await settled();
  await ersa.shot("deleted");
});
