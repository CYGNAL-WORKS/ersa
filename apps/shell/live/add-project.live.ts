import { mkdirSync } from "node:fs";
import path from "node:path";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./harness";

// Adding a project from the new-session screen (#182; FLOW-NEW-SESSION-R16 to R19) in the running app, against a
// real Core: the folder dialog is answered from the test, everything else is real.
test("Add project… opens the folder dialog, asks about trust, and cancels leave everything as it was", async ({ ersa }) => {
  const { page } = ersa;
  const stranger = path.join(ersa.dir, "stranger");
  const other = path.join(ersa.dir, "other");
  mkdirSync(stranger);
  mkdirSync(other);

  const screen = page.getByRole("region", { name: "New session" });
  const chip = screen.getByRole("button", { name: /^Project:/ });
  const words = screen.getByRole("textbox", { name: "Message" });
  const question = screen.getByRole("region", { name: /^Trust / });
  await expect(chip).toHaveAccessibleName("Project: Choose a folder");
  await words.fill("Fix the build");
  await ersa.shot("no-project");

  // The dialog canceled: nothing changes (R17).
  await chip.click();
  await expect(screen.getByRole("option", { name: /Add project…/ })).toContainText("Ctrl+O");
  await ersa.shot("chip-open");
  let answered = ersa.answerDialog(null);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  expect((await answered).options).toMatchObject({ directory: true });
  await expect(question).toHaveCount(0);
  await expect(chip).toHaveAccessibleName("Project: Choose a folder");
  await expect(words).toHaveValue("Fix the build");

  // Sending with no project opens the dialog and keeps the message; the trust question canceled changes nothing.
  // The question opens upward: the box and the session controls stay where they were (docs/design/COMPOSER.md).
  const box = await words.boundingBox();
  const controls = await screen.getByRole("group", { name: "Session controls" }).boundingBox();
  answered = ersa.answerDialog(stranger);
  await words.press("Enter");
  await answered;
  await expect(question).toHaveAccessibleName("Trust stranger?");
  expect(await words.boundingBox()).toEqual(box);
  expect(await screen.getByRole("group", { name: "Session controls" }).boundingBox()).toEqual(controls);
  await expect(question).toContainText(stranger);
  await ersa.shot("trust-question");
  await question.getByRole("button", { name: "Cancel" }).click();
  await expect(question).toHaveCount(0);
  await expect(chip).toHaveAccessibleName("Project: Choose a folder");
  await expect(words).toHaveValue("Fix the build");
  await expect(words).toBeFocused();

  // Ctrl+O, then Add, trusted: the project is added and chosen, and the message waits (R16, R19).
  answered = ersa.answerDialog(stranger);
  await words.press("Control+o");
  await answered;
  await question.getByRole("button", { name: "Add, trusted" }).click();
  await expect(chip).toHaveAccessibleName("Project: stranger");
  await expect(words).toHaveValue("Fix the build");
  await ersa.shot("added-trusted");

  // A second folder, added not trusted, then the first picked again: it is chosen without the question (R18).
  answered = ersa.answerDialog(other);
  await words.press("Control+o");
  await answered;
  await question.getByRole("button", { name: "Add, not trusted" }).click();
  await expect(chip).toHaveAccessibleName("Project: other");
  answered = ersa.answerDialog(stranger);
  await words.press("Control+o");
  await answered;
  await expect(chip).toHaveAccessibleName("Project: stranger");
  await expect(question).toHaveCount(0);
  await chip.click();
  await ersa.shot("found-without-question");
});

// The trust question in windows of every height (#218 merge review). Where the room above the box holds its title,
// folder, answers and the explanation's first line, it is capped to that room and the box stays put; where it does
// not, it takes its place in the column, the box moves down and a column taller than the window scrolls. Either way
// the title, folder and answers show together, inside the card and clear of the box, with some of the explanation.
// The sizes are measured: at 1400x700 the question fits whole above the box, at 1100x520 the cap binds, at 1100x470
// the room holds all but the explanation's first line, and the rest are short enough for the column.

/** Opens the trust question for a new folder from the project chip, as a pointer would; the first test covers Ctrl+O. */
async function askAbout(ersa: { page: Page; dir: string; answerDialog: (path: string) => Promise<unknown> }) {
  const stranger = path.join(ersa.dir, "stranger");
  mkdirSync(stranger);
  const screen = ersa.page.getByRole("region", { name: "New session" });
  await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(stranger);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  const question = screen.getByRole("region", { name: "Trust stranger?" });
  await expect(question).toBeVisible();
  // Wait out the opening motion, so the places read are where the question rests.
  await ersa.page.waitForFunction(() => document.getAnimations().length === 0, undefined, { timeout: 5_000 });
  return { stranger, question, box: screen.locator(".composer") };
}

/** The question shows its title, folder and answers together in the window, inside the card and clear of the box. */
async function showsWhole(page: Page, question: Locator, stranger: string, box: Locator) {
  // As a user would, the card's top is brought into view; a capped question needs no scrolling.
  await question.evaluate((card) => card.scrollIntoView({ block: "start" }));
  const height = page.viewportSize()!.height;
  const card = (await question.boundingBox())!;
  const boxTop = (await box.boundingBox())!.y;
  const parts = [
    question.getByText("Trust stranger?"),
    question.getByText(stranger),
    question.getByRole("button", { name: "Add, trusted" }),
    question.getByRole("button", { name: "Add, not trusted" }),
    question.getByRole("button", { name: "Cancel" }),
  ];
  for (const part of parts) {
    const at = (await part.boundingBox())!;
    expect(at.y, "in the window").toBeGreaterThanOrEqual(0);
    expect(at.y + at.height, "in the window").toBeLessThanOrEqual(height);
    expect(at.y + at.height, "inside the card").toBeLessThanOrEqual(card.y + card.height + 0.5);
    expect(at.y + at.height, "clear of the box").toBeLessThanOrEqual(boxTop);
  }
  expect(card.y + card.height, "the card clear of the box").toBeLessThanOrEqual(boxTop);
  // Some of the explanation shows: at least its first paragraph's height.
  const explained = await question.evaluate((card) => {
    const body = card.querySelector<HTMLElement>(".trust-question-body")!;
    return { shown: body.clientHeight, first: (body.firstElementChild as HTMLElement).offsetHeight };
  });
  expect(explained.shown, "the explanation's first paragraph shows").toBeGreaterThanOrEqual(explained.first);
}

for (const [width, height, stays] of [
  [1400, 700, true],
  [1100, 520, true],
  [1100, 470, false],
  [700, 520, false],
  [660, 440, false],
  [640, 400, false],
] as const) {
  test(`the trust question shows its title, folder and answers in a ${width}x${height} window`, async ({ ersa }) => {
    const { page } = ersa;
    await page.setViewportSize({ width, height });
    const before = (await page.locator(".new-session .composer").boundingBox())!;
    const { stranger, question, box } = await askAbout(ersa);
    // Where the room above the box holds the question, it opens over that room and the box stays where it was.
    if (stays) expect(await box.boundingBox()).toEqual(before);
    else expect((await box.boundingBox())!.y).toBeGreaterThan(before.y);
    await showsWhole(page, question, stranger, box);
    await ersa.shot(`trust-question-${width}x${height}`);
  });
}

test("the trust question fits again as the window shrinks, keeping where a tight screen was scrolled to", async ({ ersa }) => {
  const { page } = ersa;
  await page.setViewportSize({ width: 1400, height: 900 });
  const { stranger, question, box } = await askAbout(ersa);
  // Shrunk until the cap binds, then until the question must take its place in the column.
  for (const [width, height] of [
    [1100, 520],
    [640, 400],
  ] as const) {
    await page.setViewportSize({ width, height });
    await expect(async () => showsWhole(page, question, stranger, box)).toPass({ timeout: 5_000 });
  }
  // Scrolled down to the answers, the screen stays there when it is fitted again.
  const screen = page.locator(".new-session");
  await screen.evaluate((column) => column.scrollTo({ top: column.scrollHeight }));
  const scrolled = await screen.evaluate((column) => column.scrollTop);
  expect(scrolled).toBeGreaterThan(0);
  await page.setViewportSize({ width: 640, height: 390 });
  await expect(async () => {
    expect(await screen.evaluate((column) => column.scrollTop)).toBeGreaterThan(0);
    const cancel = (await question.getByRole("button", { name: "Cancel" }).boundingBox())!;
    expect(cancel.y + cancel.height).toBeLessThanOrEqual(390);
  }).toPass({ timeout: 5_000 });
});
