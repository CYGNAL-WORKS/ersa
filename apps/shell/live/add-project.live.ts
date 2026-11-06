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
  answered = ersa.answerDialog(stranger);
  await words.press("Enter");
  await answered;
  await expect(question).toHaveAccessibleName("Trust stranger?");
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
