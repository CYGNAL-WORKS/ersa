import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The context ring in the running window with real Claude Code (#398; PROTO-CLAUDE-CODE-STREAM-R71, R72): a new
// session's ring shows a reading before its first turn ends, from the CLI's answer at start and from each reply.
test.use({ backend: "claude" });

test("a new session's ring shows a reading before its first turn ends", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "context-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  // A long answer, so the first turn is still working while the ring is read.
  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Write a long, detailed essay of about 1500 words on the history of lighthouses. Do not use any tools.");
  await words.press("Enter");

  const composer = page.getByRole("form", { name: "Composer" });
  const stop = composer.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toBeVisible({ timeout: 120_000 });

  // The ring reads a share while the turn still works: the session controls' and the tile's carry the same name.
  const ring = page.getByRole("img", { name: /^Context: \d+ percent used$/ }).first();
  await expect(ring).toBeVisible({ timeout: 60_000 });
  await expect(stop).toBeVisible();
  await ersa.shot("reading-while-working");

  // Let the turn end on its own; the reading stays.
  await expect(composer.getByRole("button", { name: "Send", exact: true })).toBeVisible({ timeout: 240_000 });
  await expect(ring).toBeVisible();
});
