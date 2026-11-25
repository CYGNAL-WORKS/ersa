import { mkdirSync } from "node:fs";
import path from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "./harness";

// The developer pane in the running window (#569, #570; FLOW-READ-RAW-LOG-R03, R07, R10, R12, R14, R17), against the
// stand-in backend: a session's lines arrive as messages are sent and the list follows them; scrolled up, it stays where
// the user reads and counts the new lines, and Jump to latest takes it back to the end. A selected line shows in the
// detail and Copy puts it on the clipboard as it crossed; the keys move and clear the selection; the filter lists the
// lines that contain its text while more arrive.

const rail = (page: Page) => page.getByRole("navigation", { name: "Rail" });
const pane = (page: Page) => page.getByRole("complementary", { name: "Developer" });
const list = (page: Page) => pane(page).getByRole("listbox", { name: "Raw log" });

/** How far the list is from its end, in pixels. */
const fromEnd = (page: Page) => list(page).evaluate((el) => el.scrollHeight - el.clientHeight - el.scrollTop);

test("the raw log's lines arrive and the list follows them, stays once scrolled up, and Jump to latest returns", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "raw-log-check");
  mkdirSync(folder);

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail(page).getByRole("button", { name: /^New session in / })).toBeVisible();

  // On the new-session screen the pane has no session to read.
  await rail(page).getByRole("button", { name: "Arrange sessions" }).click();
  await page.getByRole("menuitemcheckbox", { name: "Developer pane" }).click();
  await expect(pane(page).getByText("No session is open. Choose a session to see its raw log.")).toBeVisible();
  await ersa.shot("no-session");

  const words = page.getByRole("textbox", { name: "Message" });
  // The stand-in's turn never ends, and a message sent meanwhile goes to it at once (FLOW-SEND-MESSAGE-R18): each is a
  // line to the backend, and `limits` has it answer with a line of its own.
  const send = async (text: string) => {
    await words.fill(text);
    await words.press("Enter");
    await expect(words).toHaveValue("", { timeout: 60_000 });
  };
  await send("First message.");
  await expect(list(page).getByRole("option").first()).toBeVisible({ timeout: 60_000 });
  // A line to the backend, heard in words.
  await expect(list(page).getByRole("img", { name: /^To / }).first()).toBeVisible();
  await ersa.shot("first-lines");

  // More messages until the lines are more than the list shows; it follows the end all the while.
  const overflows = () => list(page).evaluate((el) => el.scrollHeight > el.clientHeight * 1.2);
  for (let i = 0; i < 120 && !(await overflows()); i++) await send(i % 2 ? "limits" : `Message ${i + 2}.`);
  expect(await overflows()).toBe(true);
  await expect.poll(() => fromEnd(page), { timeout: 30_000 }).toBeLessThan(1);
  await ersa.shot("following");

  // Scrolled to the top, the list stays there as more lines arrive, and counts them.
  await list(page).hover();
  await page.mouse.wheel(0, -100_000);
  await expect.poll(() => list(page).evaluate((el) => el.scrollTop)).toBe(0);
  const jump = pane(page).getByRole("button", { name: /^Jump to latest/ });
  await expect(jump).toBeVisible();
  await send("limits");
  await expect(jump).toHaveAccessibleName(/^Jump to latest, \d+ new lines?$/, { timeout: 30_000 });
  expect(await list(page).evaluate((el) => el.scrollTop)).toBe(0);
  await ersa.shot("scrolled-away");

  // Jump to latest: back at the end, and following again.
  await jump.click();
  const tall = await list(page).evaluate((el) => el.scrollHeight);
  await expect.poll(() => fromEnd(page)).toBeLessThan(1);
  await expect(jump).toBeHidden();
  await send("limits");
  await expect.poll(() => list(page).evaluate((el) => el.scrollHeight), { timeout: 30_000 }).toBeGreaterThan(tall);
  await expect.poll(() => fromEnd(page), { timeout: 30_000 }).toBeLessThan(1);
  await ersa.shot("jumped");

  // #570 (FLOW-READ-RAW-LOG-R10, R12, R17). Select the last line: the detail shows it beneath the list. Copy, then
  // paste it into the message field: it is the line as it crossed.
  const last = list(page).getByRole("option").last();
  const crossed = (await last.locator(".raw-log-text").textContent()) ?? "";
  await last.click();
  const detail = pane(page).locator(".raw-log-detail");
  await expect(detail).toHaveAttribute("data-shown", "true");
  await ersa.shot("detail");
  await detail.getByRole("button", { name: "Copy line" }).click();
  await expect(detail.getByRole("button", { name: "Copied" })).toBeVisible();
  await words.click();
  await words.press("Control+V");
  await expect(words).toHaveValue(crossed);
  await ersa.shot("pasted");
  await words.fill("");

  // From the keyboard: Up moves the selection, and Escape closes the detail with focus left in the list.
  await list(page).focus();
  await page.keyboard.press("ArrowUp");
  await expect(list(page).getByRole("option", { selected: true })).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(detail).not.toHaveAttribute("data-shown", "true");
  await expect(list(page)).toBeFocused();

  // Filter while the session runs: only lines that contain the text, whatever its case, and more as they arrive.
  const filter = pane(page).getByRole("searchbox", { name: "Filter lines" });
  await filter.fill("LIMITS");
  const counted = pane(page).locator(".developer-count");
  await expect(counted).toHaveText(/^\d[\d,]* of \d[\d,]*$/);
  const shownNow = async () => Number((await counted.textContent())?.split(" of ")[0]?.replace(/,/g, ""));
  const before = await shownNow();
  for (const row of await list(page).getByRole("option").all()) await expect(row).toContainText(/limits/i);
  await send("limits");
  await expect.poll(shownNow, { timeout: 30_000 }).toBeGreaterThan(before);
  await ersa.shot("filtered");
  await filter.press("Escape");
  await expect(filter).toHaveValue("");
});
