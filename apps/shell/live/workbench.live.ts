import type { Page } from "@playwright/test";
import { expect, test } from "./harness";

// The workbench in the running window (#567; FLOW-READ-RAW-LOG-R01, R02, R16): opened from Developer pane in the
// arrange menu, resized by dragging its edge and from the keyboard, and open at that width after Ersa and Core start
// again.

const rail = (page: Page) => page.getByRole("navigation", { name: "Rail" });
const workbench = (page: Page) => page.getByRole("complementary", { name: "Developer" });
const edge = (page: Page) => page.getByRole("separator", { name: "Resize workbench" });

test("the workbench opens, resizes, and opens again at that width after a restart", async ({ ersa }) => {
  test.setTimeout(180_000);
  let page = ersa.page;
  const arrange = rail(page).getByRole("button", { name: "Arrange sessions" });
  await expect(arrange).toBeEnabled({ timeout: 60_000 });
  await expect(workbench(page)).toHaveCount(0);

  await arrange.click();
  await page.getByRole("menuitemcheckbox", { name: "Developer pane" }).click();
  await expect(workbench(page)).toBeVisible();
  // Grown from nothing over motion-settle, it rests at the design's width.
  await expect(edge(page)).toHaveAttribute("aria-valuenow", "480");
  await expect.poll(async () => Math.round((await workbench(page).boundingBox())?.width ?? 0)).toBe(480);
  await ersa.shot("opened");

  // Dragged 100 px toward the transcript, it is 100 px wider.
  const box = (await edge(page).boundingBox())!;
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x - 50, y);
  await page.mouse.move(x - 100, y);
  await page.mouse.up();
  await expect(edge(page)).toHaveAttribute("aria-valuenow", "580");
  // Then one step back from the keyboard.
  await edge(page).focus();
  await page.keyboard.press("ArrowRight");
  await expect(edge(page)).toHaveAttribute("aria-valuenow", "564");
  await expect.poll(async () => Math.round((await workbench(page).boundingBox())?.width ?? 0)).toBe(564);
  await ersa.shot("resized");

  await ersa.restart();
  page = ersa.page;
  await expect(workbench(page)).toBeVisible({ timeout: 60_000 });
  await expect(edge(page)).toHaveAttribute("aria-valuenow", "564");
  await expect.poll(async () => Math.round((await workbench(page).boundingBox())?.width ?? 0)).toBe(564);
  await rail(page).getByRole("button", { name: "Arrange sessions" }).click();
  await expect(page.getByRole("menuitemcheckbox", { name: "Developer pane" })).toHaveAttribute("aria-checked", "true");
  await ersa.shot("after-restart");

  // Closed from its Close, focus goes to the arrange button.
  await page.keyboard.press("Escape");
  await workbench(page).getByRole("button", { name: "Close" }).click();
  await expect(workbench(page)).toHaveCount(0);
  await ersa.shot("closed");
});
