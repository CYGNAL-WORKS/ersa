import { mkdirSync } from "node:fs";
import path from "node:path";
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

// In a short window the question is capped to the room above the box, or takes its place in the column when even its
// title, folder and answers would not fit there: either way the folder being trusted is in view (#218 merge review).
for (const [width, height] of [[700, 520], [660, 440], [640, 400]] as const) {
  test(`the trust question shows its title, folder and answers in a ${width}x${height} window`, async ({ ersa }) => {
    const { page } = ersa;
    await page.setViewportSize({ width, height });
    const stranger = path.join(ersa.dir, "stranger");
    mkdirSync(stranger);
    const screen = page.getByRole("region", { name: "New session" });
    // From the chip, as a pointer would; the first test covers Ctrl+O.
    await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
    const answered = ersa.answerDialog(stranger);
    await screen.getByRole("option", { name: /Add project…/ }).click();
    await answered;
    const question = screen.getByRole("region", { name: "Trust stranger?" });
    await expect(question).toBeVisible();
    // Wait out the opening motion, so the places read are where the question rests.
    await page.waitForFunction(() => document.getAnimations().length === 0);
    const inView = async (part: import("@playwright/test").Locator) => {
      await part.scrollIntoViewIfNeeded();
      const box = (await part.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(0);
      expect(box.y + box.height).toBeLessThanOrEqual(height);
    };
    await inView(question.getByText("Trust stranger?"));
    // The folder and the answers must show together with the title, without scrolling between them.
    const title = (await question.getByText("Trust stranger?").boundingBox())!;
    for (const part of [question.getByText(stranger), question.getByRole("button", { name: "Add, trusted" }), question.getByRole("button", { name: "Cancel" })]) {
      const box = (await part.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(title.y);
      expect(box.y + box.height).toBeLessThanOrEqual(height);
    }
    await ersa.shot(`trust-question-${width}x${height}`);
  });
}
