import { expect, test } from "./harness";

// The effort chip against the real Claude Code (#587; PROTO-CLAUDE-CODE-STREAM-R82, FLOW-NEW-SESSION-R47): the model
// probe asks the CLI's `get_settings` which effort a session given none runs with, and the new-session screen's effort
// chip reads that level, with no Default row. It sends no message, so it spends no usage.
test.use({ backend: "claude" });

test("the new-session screen's effort chip reads the level Claude Code applies", async ({ ersa }) => {
  test.setTimeout(180_000);
  const { page } = ersa;
  const effort = page.getByRole("button", { name: /^Effort:/ });
  await expect(effort).toBeVisible({ timeout: 90_000 });
  // The level `get_settings` reported, as Core marks it in `backend.list`.
  const marked = await page.evaluate(async () => {
    const hosts = (await (window as any).__TAURI_INTERNALS__.invoke("core_request", { method: "backend.list", params: { host_id: "local" } })) as { name: string; efforts: { label: string; default: boolean }[] }[];
    return hosts.find((b) => b.name === "claude-code")?.efforts.filter((e) => e.default).map((e) => e.label);
  });
  expect(marked).toHaveLength(1);
  await expect(effort).toHaveAccessibleName(`Effort: ${marked?.[0]}`);
  await effort.click();
  const list = page.getByRole("listbox", { name: "Effort" });
  await expect(list.getByRole("option", { name: "Default" })).toHaveCount(0);
  await expect(list.getByRole("option", { selected: true })).toHaveText(marked?.[0] ?? "");
  await page.keyboard.press("Escape");
});
