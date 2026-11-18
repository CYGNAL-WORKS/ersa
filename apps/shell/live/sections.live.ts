import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, otherWindow, test } from "./harness";

// Grouping by section in the running window (#326; FLOW-ORGANIZE-SESSIONS-R04, R05), on the stand-in backend: every
// section Core has shows, empty or not, in the order made, then No section; a heading's Rename works in place; Delete
// asks nothing and keeps every session, under No section. This window has no way to make a section yet (#332), so
// another window, as Core sees one, makes them.
test("grouped by section, a heading renames in place and deletes keeping its sessions", async ({ ersa }) => {
  test.setTimeout(180_000);
  const { page, label } = ersa;
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

  const docs = (await otherWindow(label, "section.create", { name: "Docs" })) as { id: string };
  await otherWindow(label, "section.create", { name: "Later" });
  await otherWindow(label, "session.arrange", { id, section_id: docs.id });
  // The window reads the sections as it connects.
  await page.reload();

  await rail.getByRole("button", { name: "Arrange sessions" }).click();
  await page.getByRole("menuitem", { name: /^Group by/ }).click();
  await page.getByRole("menuitemradio", { name: "Section" }).click();
  const names = rail.locator(".rail-projects > .rail-project .rail-heading-name");
  await expect(names).toHaveText(["Docs", "Later"]);
  await settled();
  await ersa.shot("grouped-by-section");

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
