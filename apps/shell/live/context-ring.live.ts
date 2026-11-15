import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The context ring (#410; LAY-SHELL, Tiles and Context) in the running app, against a real Core and the stand-in
// backend: before a session the ring says none is used; a new session's tile shows the ring from the start, saying
// what it reads, and its session controls show the same ring with the same words.
test("a new session's tile and session controls always show the context ring", async ({ ersa }) => {
  const { page } = ersa;
  const folder = path.join(ersa.dir, "ring-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  const screen = page.getByRole("region", { name: "New session" });
  await expect(screen.getByRole("img", { name: "Context: none used yet" })).toBeVisible();
  await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Hello");
  await words.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 60_000 });

  // Whatever the backend has reported so far, the ring is there and names it: not reported yet, or a share.
  const named = /^Context( isn't reported yet|: (less than 1|\d+) percent used)$/;
  const tileRing = rail.getByRole("img", { name: named });
  await expect(tileRing).toHaveCount(1);
  const controlsRing = page.getByRole("group", { name: "Session controls" }).getByRole("img", { name: named });
  await expect(controlsRing).toBeVisible();
  // The same words on the tile and in the session controls, once both have drawn the same reading.
  await expect(async () => expect(await tileRing.getAttribute("aria-label")).toBe(await controlsRing.getAttribute("aria-label"))).toPass();
  // Under the pointer it says what it says to assistive technology.
  await expect(controlsRing).toHaveAttribute("title", named);
  await ersa.shot("ring");
});
