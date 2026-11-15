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
  // Under the pointer it lists its one reading, the context, as it says it to assistive technology.
  await controlsRing.hover();
  await expect(page.getByRole("tooltip")).toHaveText(named);
  await ersa.shot("ring");
});

// The plan's usage limits on the ring (#422; LAY-SHELL, Context; COMPOSER.md, The context ring): once the backend
// reports its usage windows (the stand-in's made-up ones, on the message "limits"), the session controls' ring reads
// the nearest limit and names it, lists every reading under the pointer and with keyboard focus, and the tile's
// ring still reads the context. In both themes.
test("the session controls' ring reads the nearest limit and lists every reading", async ({ ersa }) => {
  const { page } = ersa;
  const folder = path.join(ersa.dir, "limits-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });
  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("limits");
  await words.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 60_000 });

  const controls = page.getByRole("group", { name: "Session controls" });
  const ring = controls.getByRole("img", { name: "5-hour limit: 84 percent used" });
  await expect(ring).toBeVisible({ timeout: 30_000 });
  await expect(ring).toHaveAttribute("data-band", "danger");
  await expect(ring).toHaveText("84%");
  // The tile's ring reads the context alone.
  await expect(rail.getByRole("img", { name: /^Context/ })).toHaveCount(1);
  await expect(rail.getByRole("img", { name: /limit/ })).toHaveCount(0);

  const tip = page.getByRole("tooltip");
  const lines = /^Context( isn't reported yet|: (less than 1|\d+) percent used)5-hour limit: 84 percent usedResets in 1 hr 1[45] minWeekly · all models: 62 percent usedResets \w{3} \d{1,2}:\d{2}\s[AP]M$/;
  // With keyboard focus, then under the pointer, in light and in dark.
  await ring.focus();
  await page.keyboard.press("Shift+Tab");
  await page.keyboard.press("Tab");
  await expect(ring).toBeFocused();
  await expect(tip).toBeVisible();
  await expect(tip).toHaveText(lines);
  await ersa.shot("limits-focus-light");
  await page.emulateMedia({ colorScheme: "dark" });
  await ersa.shot("limits-focus-dark");
  await page.getByRole("textbox", { name: "Message" }).focus();
  await expect(tip).toBeHidden();
  await ring.hover();
  await expect(tip).toBeVisible();
  await ersa.shot("limits-hover-dark");
  await page.emulateMedia({ colorScheme: "light" });
  await ersa.shot("limits-hover-light");
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
