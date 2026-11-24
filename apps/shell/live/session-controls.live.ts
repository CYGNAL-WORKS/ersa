import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// A running session's controls (#339; FLOW-NEW-SESSION-R30) in the running app, against a real Core and the stand-in
// backend, which accepts each change as Claude Code does: the permission mode and effort chips open under the session,
// and each reads the new value once Core has it, with keyboard focus kept on the chip.
test("a running session's permission mode and effort change from its session controls", async ({ ersa }) => {
  const { page } = ersa;
  const folder = path.join(ersa.dir, "controls-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  const screen = page.getByRole("region", { name: "New session" });
  await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Hello");
  await words.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 60_000 });

  const controls = page.getByRole("group", { name: "Session controls" });
  const mode = controls.getByRole("button", { name: /^Permission mode:/ });
  await expect(mode).toHaveAccessibleName("Permission mode: Manual");
  await expect(mode).toBeEnabled();
  // The where chips stay read-only under the session.
  await expect(controls.getByRole("button", { name: /^Project:/ })).toBeDisabled();

  await mode.click();
  await ersa.shot("permission-mode-open");
  await page.getByRole("option", { name: "Plan", exact: true }).click();
  await expect(mode).toHaveAccessibleName("Permission mode: Plan", { timeout: 30_000 });
  await expect(mode).toBeFocused();

  const effort = controls.getByRole("button", { name: /^Effort:/ });
  await effort.click();
  // A running session's effort list holds only the levels.
  await expect(page.getByRole("option")).toHaveText(["Low", "High"]);
  await page.getByRole("option", { name: "High", exact: true }).click();
  await expect(effort).toHaveAccessibleName("Effort: High", { timeout: 30_000 });
  await expect(page.getByRole("alert")).toHaveCount(0);
  await ersa.shot("changed");
});
