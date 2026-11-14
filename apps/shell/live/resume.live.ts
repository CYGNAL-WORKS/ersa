import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { expect, test } from "./harness";

// Resume in the running window with real Claude Code (#321; FLOW-RESUME-SESSION-R01, R02, R08): a session stopped by
// quitting Ersa, and written to after Ersa is opened again, continues in the same record.
test.use({ backend: "claude" });

test("a session stopped by quitting Ersa continues in the same record when written to after reopening", async ({ ersa }) => {
  test.setTimeout(300_000);
  const folder = path.join(ersa.dir, "resume-check");
  mkdirSync(folder);

  let page = ersa.page;
  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(page.getByRole("navigation", { name: "Rail" }).getByRole("button", { name: /^New session in / })).toBeVisible();

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Reply with only the word one.");
  await words.press("Enter");
  let transcript = page.getByRole("region", { name: "Transcript" });
  await expect(transcript.getByText("one", { exact: true })).toBeVisible({ timeout: 120_000 });
  await ersa.shot("first-turn");

  // Quit and open again: the session's backend ended with Ersa.
  await ersa.restart();
  page = ersa.page;
  const rail = page.getByRole("navigation", { name: "Rail" });
  await rail.getByRole("button", { name: /word one/ }).first().click();
  transcript = page.getByRole("region", { name: "Transcript" });
  await expect(transcript.getByText("one", { exact: true })).toBeVisible({ timeout: 60_000 });
  await ersa.shot("reopened");

  // Writing to it resumes it.
  const again = page.getByRole("textbox", { name: "Message" });
  await again.fill("Reply with only the word two.");
  await again.press("Enter");
  await expect(transcript.getByText("two", { exact: true })).toBeVisible({ timeout: 120_000 });
  await expect(transcript.getByText("one", { exact: true })).toBeVisible();
  await ersa.shot("resumed");

  // One record holds both turns: the CLI's own session file for the folder.
  const records = path.join(homedir(), ".claude", "projects", folder.replace(/[^A-Za-z0-9]/g, "-"));
  const files = readdirSync(records).filter((f) => f.endsWith(".jsonl"));
  const both = files.filter((f) => {
    const text = readFileSync(path.join(records, f), "utf8");
    return text.includes("Reply with only the word one.") && text.includes("Reply with only the word two.");
  });
  expect(files).toHaveLength(1);
  expect(both).toHaveLength(1);
});
