import { existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The shelf in the running window (#176): a real Claude Code session asks for permission, another session is made
// active, and the first one's chevron opens its shelf beside its tile over the transcript, in both themes.
test.use({ backend: "claude" });

test("the chevron opens the shelf with the oldest permission request, leaving the active session as it was", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "shelf-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  await rail.getByRole("textbox", { name: "Project folder" }).fill(folder);
  await rail.getByRole("button", { name: "Add", exact: true }).click();
  await page.getByRole("button", { name: "Add, not trusted" }).click();

  // The first session asks to write a file, which Claude Code's default permission mode asks about.
  await rail.getByRole("button", { name: "New session" }).click();
  await page.getByRole("textbox", { name: "Message" }).fill("Use the Write tool to create shelf-check.txt containing the word probe. Do nothing else.");
  await page.keyboard.press("Enter");
  // While it is the active session, its chevron is a mark: its transcript answers it.
  await expect(rail.getByText("Needs permission")).toBeVisible({ timeout: 180_000 });
  await expect(rail.getByRole("button", { name: /^Request from / })).toHaveCount(0);

  // A second session becomes the active one.
  await rail.getByRole("button", { name: "New session" }).click();
  await page.getByRole("textbox", { name: "Message" }).fill("Reply with the single word ready.");
  await page.keyboard.press("Enter");
  const chevron = rail.getByRole("button", { name: /^Request from / });
  await expect(chevron).toBeVisible({ timeout: 60_000 });
  await expect(chevron).toHaveAttribute("aria-expanded", "false");
  const transcript = page.getByRole("region", { name: "Transcript" });
  await expect(transcript).toBeVisible();
  const before = await transcript.boundingBox();

  await chevron.click();
  const shelf = page.getByRole("dialog", { name: /^Request from / });
  await expect(shelf).toBeVisible();
  await expect(chevron).toHaveAttribute("aria-expanded", "true");
  await expect(shelf.getByText("Permission request")).toBeVisible({ timeout: 30_000 });
  await expect(shelf).toContainText("shelf-check.txt");
  await expect(shelf.getByRole("button", { name: "Allow" })).toBeVisible();
  // The active session is unchanged, and the transcript did not move or resize (R01, R04).
  await expect(transcript).toContainText("Reply with the single word ready.");
  await expect(transcript).not.toContainText("shelf-check.txt");
  expect(await transcript.boundingBox()).toEqual(before);
  // The shelf lies beside its tile, over the transcript's edge.
  const shelfBox = (await shelf.boundingBox())!;
  const railBox = (await rail.boundingBox())!;
  expect(shelfBox.x).toBeLessThan(railBox.x + railBox.width);
  expect(shelfBox.x + shelfBox.width).toBeGreaterThan(railBox.x + railBox.width);
  await page.waitForTimeout(500);
  await ersa.shot("shelf-light");
  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForTimeout(300);
  await ersa.shot("shelf-dark");
  await page.emulateMedia({ colorScheme: "light" });

  // Choosing the chevron again closes it; opening it and then making its session active closes it too (R26).
  await chevron.click();
  // It slides back and fades before it goes, out of reach meanwhile.
  const closing = page.locator(".shelf-layer[inert] > .shelf");
  await expect(closing).toHaveCount(1);
  expect(await closing.evaluate((el) => getComputedStyle(el).animationName)).toBe("shelf-back");
  // The same element that was open, so it keeps the join measured beside its tile as it goes.
  expect(await closing.evaluate((el) => el.style.getPropertyValue("--shelf-join-height"))).toMatch(/^[1-9]/);
  await expect(closing).toHaveCount(0);
  await expect(shelf).toHaveCount(0);
  await chevron.click();
  await expect(shelf).toBeVisible();
  await rail.getByRole("button", { name: /Needs permission/ }).click();
  await expect(shelf).toHaveCount(0);
  await expect(rail.getByRole("button", { name: /^Request from / })).toHaveCount(0);
  await ersa.shot("made-active");
});

test("an answer from the shelf reaches the session, which goes on, and the shelf closes once nothing is pending", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "shelf-answer");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  await rail.getByRole("textbox", { name: "Project folder" }).fill(folder);
  await rail.getByRole("button", { name: "Add", exact: true }).click();
  await page.getByRole("button", { name: "Add, not trusted" }).click();

  await rail.getByRole("button", { name: "New session" }).click();
  await page.getByRole("textbox", { name: "Message" }).fill("Use the Write tool to create answered.txt containing the word probe. Do nothing else.");
  await page.keyboard.press("Enter");
  await expect(rail.getByText("Needs permission")).toBeVisible({ timeout: 180_000 });

  await rail.getByRole("button", { name: "New session" }).click();
  await page.getByRole("textbox", { name: "Message" }).fill("Reply with the single word ready.");
  await page.keyboard.press("Enter");
  const chevron = rail.getByRole("button", { name: /^Request from / });
  await expect(chevron).toBeVisible({ timeout: 60_000 });
  await chevron.click();
  const shelf = page.getByRole("dialog", { name: /^Request from / });
  await expect(shelf.getByText("Permission request")).toBeVisible({ timeout: 30_000 });
  // Claude Code offers a suggestion for a write; the shelf offers it beside Deny and Allow (R12).
  await expect(shelf.getByRole("button")).not.toHaveCount(2);
  await ersa.shot("shelf-answers");

  await shelf.getByRole("button", { name: "Allow", exact: true }).click();
  // The session goes on, its request answered: the shelf closes, and the file is written (R13).
  await expect(chevron).toHaveCount(0, { timeout: 60_000 });
  await expect(page.locator(".shelf")).toHaveCount(0);
  await expect.poll(() => existsSync(path.join(folder, "answered.txt")), { timeout: 120_000 }).toBe(true);
  await ersa.shot("shelf-answered");
});
