import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Stop in the running window with real Claude Code (#188; FLOW-SEND-MESSAGE-R03, R04): send becomes stop while the
// agent works, Enter does not send meanwhile, and stop ends the turn as interrupted with the words kept.
test.use({ backend: "claude" });

test("stop ends a working turn as interrupted, and what was written meanwhile stays in the composer", async ({ ersa }) => {
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

  // R04: Enter does not send while the agent works, and the words stay.
  await words.fill("Then summarise it in one line.");
  await words.press("Enter");
  await expect(words).toHaveValue("Then summarise it in one line.");
  await ersa.shot("working");

  // R03: stop interrupts the turn, which ends as interrupted; send comes back with focus, and the words stay.
  await stop.focus();
  await page.keyboard.press("Enter");
  await expect(transcript.getByText("The turn was interrupted.")).toBeVisible({ timeout: 60_000 });
  const send = composer.getByRole("button", { name: "Send", exact: true });
  await expect(send).toBeVisible();
  await expect(send).toBeFocused();
  await expect(words).toHaveValue("Then summarise it in one line.");
  await ersa.shot("interrupted");
});
