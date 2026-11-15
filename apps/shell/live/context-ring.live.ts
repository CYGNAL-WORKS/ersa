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

// With real Claude Code (#398; PROTO-CLAUDE-CODE-STREAM-R71, R72): a new session's ring shows a reading before its
// first turn ends, from the CLI's answer at start and from each reply.
test.describe("with Claude Code", () => {
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

    // The session controls' ring reads a share while the turn still works.
    const ring = page.getByRole("group", { name: "Session controls" }).getByRole("img", { name: /^Context: (less than 1|\d+) percent used$/ });
    await expect(ring).toBeVisible({ timeout: 60_000 });
    await expect(stop).toBeVisible();
    await ersa.shot("reading-while-working");

    // Let the turn end on its own; the reading stays.
    await expect(composer.getByRole("button", { name: "Send", exact: true })).toBeVisible({ timeout: 240_000 });
    await expect(ring).toBeVisible();
  });
});
