import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The effort chip reads the backend's default effort (#587; FLOW-NEW-SESSION-R47, PROTO-CLAUDE-CODE-STREAM-R82), in the
// running app against a real Core and the stand-in backend, which says through `get_settings` that a session given no
// effort runs at Low: the chip reads Low, its list holds the levels alone with Low marked, and a session started
// without choosing one is created with no effort.
test("with no effort chosen the chip reads the backend's default, and the session names none", async ({ ersa }) => {
  const { page } = ersa;
  const folder = path.join(ersa.dir, "effort-check");
  mkdirSync(folder);

  const screen = page.getByRole("region", { name: "New session" });
  await screen.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await screen.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(page.getByRole("button", { name: "Project: effort-check" })).toBeVisible();

  const effort = page.getByRole("button", { name: /^Effort:/ });
  await expect(effort).toHaveAccessibleName("Effort: Low", { timeout: 60_000 });
  await effort.click();
  const list = page.getByRole("listbox", { name: "Effort" });
  await expect(list.getByRole("option")).toHaveText(["Low", "High"]);
  await expect(list.getByRole("option", { selected: true })).toHaveText("Low");
  await ersa.shot("effort-list");
  await page.keyboard.press("Escape");
  await expect(effort).toBeFocused();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Hello");
  await words.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 60_000 });
  // Core reports the effort it was asked for until the backend reports its own, and the stand-in reports none: an
  // absent effort is a session created with none.
  const listed = await page.evaluate(async () =>
    ((await (window as any).__TAURI_INTERNALS__.invoke("core_request", { method: "session.list", params: null })) as { effort?: string }[]).map((s) => ({ effort: s.effort })),
  );
  expect(listed).toEqual([{ effort: undefined }]);
  // Under the session, which reports none, the chip still reads the default.
  await expect(page.getByRole("group", { name: "Session controls" }).getByRole("button", { name: /^Effort:/ })).toHaveAccessibleName("Effort: Low");
  await ersa.shot("session-started");
});
