import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Stop in the running window with real Claude Code (#188, #338; FLOW-SEND-MESSAGE-R03, R18 to R20): send becomes stop
// while the agent works, Enter sends meanwhile and the message waits, named on the status line, and stop ends only the
// running turn, as interrupted, the waiting message running next.
test.use({ backend: "claude" });

test("a message sent while the agent works waits, and stop ends only the running turn, the message running next", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "stop-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  // A long answer, so the turn is still working when stop is chosen.
  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Write a long, detailed essay of about 1500 words on the history of lighthouses. Do not use any tools.");
  await words.press("Enter");

  // Under the session, the turn works: the button is stop.
  const transcript = page.getByRole("region", { name: "Transcript" });
  await expect(transcript).toBeVisible({ timeout: 120_000 });
  const composer = page.getByRole("form", { name: "Composer" });
  const stop = composer.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toBeVisible({ timeout: 120_000 });

  // R18: Enter sends while the agent works, and the composer clears once the session accepts it; stop stays.
  await words.fill("Then summarise it in one line.");
  await words.press("Enter");
  await expect(words).toHaveValue("", { timeout: 60_000 });
  await expect(stop).toBeVisible();
  // R19: the status line names it while it waits in the backend.
  const status = page.locator(".composer-status-line");
  await expect(status).toHaveText("Waiting: Then summarise it in one line.", { timeout: 60_000 });
  await ersa.shot("waiting");

  // R20: stop interrupts only the running turn, which ends as interrupted, and the waiting message runs next.
  await stop.focus();
  await page.keyboard.press("Enter");
  await expect(transcript.getByText("The turn was interrupted.")).toBeVisible({ timeout: 60_000 });
  await expect(transcript.getByText("Then summarise it in one line.")).toBeVisible({ timeout: 120_000 });
  await expect(status).toHaveCount(0, { timeout: 120_000 });
  // Its turn ends on its own; send comes back with focus kept.
  const send = composer.getByRole("button", { name: "Send", exact: true });
  await expect(send).toBeVisible({ timeout: 180_000 });
  await expect(send).toBeFocused();
  await ersa.shot("ran-next");
});
