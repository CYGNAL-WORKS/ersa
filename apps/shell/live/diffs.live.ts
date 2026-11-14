import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Diff cards in the running window (#333): a real Claude Code turn edits a file and writes a new one; each call shows
// as a diff card, open from the start, waiting for the user with its change in view, then done with its counts.
test.use({ backend: "claude" });

test("an edit and a new file show as diff cards, with the change in view while they wait for the user", async ({ ersa }) => {
  test.setTimeout(420_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "diffs-check");
  mkdirSync(path.join(folder, "docs"), { recursive: true });
  writeFileSync(path.join(folder, "docs", "notes.txt"), "First line.\nThe diff card check changes this line.\nLast line.\n");
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await rail.getByRole("button", { name: /^New session in / }).click();
  await page
    .getByRole("textbox", { name: "Message" })
    .fill(
      [
        "In this order, and with no other tools:",
        "1. Use the Edit tool on docs/notes.txt to replace the text `The diff card check changes this line.` with `The diff card check changed this line.`",
        "2. Use the Write tool to create docs/new.txt holding exactly two lines: `One` and `Two`.",
        "Then reply with one short sentence.",
      ].join("\n"),
    );
  await page.keyboard.press("Enter");

  const transcript = page.getByRole("region", { name: "Transcript" });
  // The edit waits for the user, open on its change, the counts on its row.
  const edit = transcript.getByRole("button", { name: /^Edit docs[\\/]notes\.txt, Waiting for you, 1 line added, 1 line removed$/ });
  await expect(edit).toBeVisible({ timeout: 240_000 });
  await expect(edit).toHaveAttribute("aria-expanded", "true");
  const editCard = edit.locator("xpath=..");
  await expect(editCard.locator(".diff-line[data-sign='removed'] .diff-text")).toHaveText(["The diff card check changes this line."]);
  await expect(editCard.locator(".diff-line[data-sign='added'] .diff-text")).toHaveText(["The diff card check changed this line."]);
  await ersa.shot("edit-waiting-light");
  await transcript.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(transcript.getByRole("button", { name: /^Edit docs[\\/]notes\.txt, Done, 1 line added, 1 line removed$/ })).toBeVisible({ timeout: 60_000 });

  // The new file: all added lines.
  const write = transcript.getByRole("button", { name: /^Write docs[\\/]new\.txt, Waiting for you, 2 lines added, 0 lines removed$/ });
  await expect(write).toBeVisible({ timeout: 180_000 });
  await expect(write.locator("xpath=..").locator(".diff-line .diff-text")).toHaveText(["One", "Two"]);
  await transcript.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(transcript.getByRole("button", { name: /^Write docs[\\/]new\.txt, Done, 2 lines added, 0 lines removed$/ })).toBeVisible({ timeout: 60_000 });
  await page.mouse.move(0, 0);
  await page.waitForTimeout(500);
  await ersa.shot("diffs-done-light");
  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForTimeout(300);
  await ersa.shot("diffs-done-dark");
});
