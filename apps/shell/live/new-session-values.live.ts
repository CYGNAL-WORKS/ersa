import { execFileSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import path from "node:path";
import type { Page } from "@playwright/test";
import { expect, test } from "./harness";

// The values chosen on the new-session screen with real Claude Code (#397; FLOW-NEW-SESSION-R02, R12, R19): chosen
// before the project is added, they are the ones the session starts with, the CLI runs with, and the session controls
// show, under the labels chosen, after the first turn, when Claude Code reports an alias by the id it resolved to.
test.use({ backend: "claude" });

/** What the session controls read, chip by chip. */
const reads = (page: Page, chips: string[]) =>
  Promise.all(chips.map((chip) => page.getByRole("button", { name: new RegExp(`^${chip}:`) }).getAttribute("aria-label")));

/** The arguments of the Claude Code processes this test's own Core started, found by its endpoint label. */
function invocations(label: string): string[] {
  const script = `
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, CommandLine
    $ids = @($all | Where-Object { $_.CommandLine -like '*--endpoint ${label}*' } | ForEach-Object { $_.ProcessId })
    do { $more = @($all | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId } | ForEach-Object { $_.ProcessId }); $ids += $more } while ($more.Count -gt 0)
    $all | Where-Object { $ids -contains $_.ProcessId -and $_.CommandLine -like '*--await-initialize*' } | ForEach-Object { $_.CommandLine }`;
  return execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" })
    .split(/\r?\n/)
    .filter((line) => line.includes("--await-initialize"));
}

test("a session runs with, and shows, the model, effort and permission mode chosen before its project was added", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "values-check");
  mkdirSync(folder);
  const chips = ["Model", "Effort", "Permission mode"];
  const choose = async (chip: string, option: string) => {
    await page.getByRole("button", { name: new RegExp(`^${chip}:`) }).click();
    await page.getByRole("listbox", { name: chip }).getByRole("option", { name: option, exact: true }).click();
  };

  // An alias (`opus`), which Claude Code reports by the full id once a turn has run, and values none of which is a default.
  await expect(page.getByRole("button", { name: /^Model:/ })).toBeVisible({ timeout: 60_000 });
  await choose("Model", "Opus 5.5");
  await choose("Effort", "Low");
  await choose("Permission mode", "Plan");
  const chosen = ["Model: Opus 5.5", "Effort: Low", "Permission mode: Plan"];
  expect(await reads(page, chips)).toEqual(chosen);

  // R19: then the project, added from the project chip.
  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(page.getByRole("button", { name: "Project: values-check" })).toBeVisible();
  expect(await reads(page, chips)).toEqual(chosen);
  await ersa.shot("chosen");

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Reply with the single word: ready. Do not use any tools.");
  await words.press("Enter");
  await expect(page.getByRole("region", { name: "Transcript" })).toBeVisible({ timeout: 120_000 });
  const [cli] = invocations(ersa.label);
  expect(cli).toMatch(/--permission-mode plan\b/);
  expect(cli).toMatch(/--model opus\b/);
  expect(cli).toMatch(/--effort low\b/);

  // After the first turn: the CLI has reported the model it resolved `opus` to, which Core lists as `opus`, and the
  // chips still read as chosen.
  await expect(page.getByRole("form", { name: "Composer" }).getByRole("button", { name: "Send", exact: true })).toBeVisible({ timeout: 120_000 });
  const listed = await page.evaluate(async () => {
    const all = (await (window as any).__TAURI_INTERNALS__.invoke("core_request", { method: "session.list", params: null })) as { model?: string; effort?: string; permission_mode: string }[];
    return all.map(({ model, effort, permission_mode }) => ({ model, effort, permission_mode }));
  });
  expect(listed).toEqual([{ model: "opus", effort: "low", permission_mode: "plan" }]);
  expect(await reads(page, chips)).toEqual(chosen);
  await ersa.shot("after-first-turn");
});
