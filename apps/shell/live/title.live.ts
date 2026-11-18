import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, otherWindow, test } from "./harness";

// A session's title in the running window with real Claude Code (#329; PROTO-CORE-API-R83, R84): a new session's tile
// shows the name the backend generated during its first turn, with no reload, and a rename from another client shows at
// once, on the tile and in the transcript header.
test.use({ backend: "claude" });

test("a new session's generated name and a rename from another window show without a reload", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "title-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Write a short haiku about lighthouses. Do not use any tools.");
  await words.press("Enter");

  // The backend names the session during its first turn; the tile shows it with nothing reloaded.
  const tile = rail.locator("[data-session-id]").first();
  await expect(tile).toBeVisible({ timeout: 120_000 });
  await expect(tile).not.toContainText("Untitled session", { timeout: 120_000 });
  await ersa.shot("generated-name");
  const id = await tile.getAttribute("data-session-id");

  // Renamed from another window: this window shows it at once, on the tile and in the header.
  await otherWindow(ersa.label, "session.set_title", { id, title: "Renamed elsewhere" });
  await expect(tile).toContainText("Renamed elsewhere", { timeout: 30_000 });
  await expect(page.getByRole("heading", { name: "Renamed elsewhere" })).toBeVisible();
  await ersa.shot("renamed-elsewhere");
});
