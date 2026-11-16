import { spawnSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { expect, test } from "./harness";

// Sessions started outside Ersa with real Claude Code (#328; FLOW-RESUME-SESSION-R12, R01): a session the CLI ran in a
// folder shows in the rail once the folder is added as a project, and continues in the same record when written to.
test.use({ backend: "claude" });

test("a session started in the CLI in a project's folder shows in the rail and resumes when written to", async ({ ersa }) => {
  test.setTimeout(300_000);
  const folder = path.join(ersa.dir, "outside-check");
  mkdirSync(folder);

  // The CLI's own run, in print mode: one reply, no tools, so nothing asks for permission. Variables that tie a
  // process to a Claude session this test may run inside are left out, as the adapter leaves them out.
  const env = { ...process.env };
  for (const name of Object.keys(env)) {
    if (name === "CLAUDECODE" || name.startsWith("CLAUDE_CODE_") || name === "CLAUDE_AGENT_SDK_VERSION") delete env[name];
  }
  const run = spawnSync('claude -p "Reply with only the word outside."', { cwd: folder, env, shell: true, encoding: "utf8", timeout: 120_000 });
  expect(run.status, run.stderr).toBe(0);

  const page = ersa.page;
  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();

  const rail = page.getByRole("navigation", { name: "Rail" });
  const found = rail.getByRole("button", { name: /word outside/ }).first();
  await expect(found).toBeVisible({ timeout: 30_000 });
  await ersa.shot("listed");
  await found.click();
  const transcript = page.getByRole("region", { name: "Transcript" });
  await expect(transcript.getByText("outside", { exact: true })).toBeVisible({ timeout: 60_000 });
  await ersa.shot("opened");

  const words = page.getByRole("textbox", { name: "Message" });
  await words.fill("Reply with only the word inside.");
  await words.press("Enter");
  await expect(transcript.getByText("inside", { exact: true })).toBeVisible({ timeout: 120_000 });
  await ersa.shot("resumed");

  // One record holds both turns: the CLI's own session file for the folder.
  const records = path.join(homedir(), ".claude", "projects", folder.replace(/[^A-Za-z0-9]/g, "-"));
  const files = readdirSync(records).filter((f) => f.endsWith(".jsonl"));
  expect(files).toHaveLength(1);
  const text = readFileSync(path.join(records, files[0]!), "utf8");
  expect(text).toContain("Reply with only the word outside.");
  expect(text).toContain("Reply with only the word inside.");
});
