import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The attach menu in the running window with real Claude Code (#191; FLOW-SEND-MESSAGE-R07, R09, R12, R13, R17): each
// choice opens the system's dialog, the paths picked are attached once with their detail, a file from outside the
// workspace says it will be copied while an image, which goes inline, does not, and the agent reads them.
test.use({ backend: "claude" });

// A 1 by 1 PNG.
const PNG = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==", "base64");

test("attaches files, an image and a folder from the attach menu, and the agent reads them", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "attach-check");
  mkdirSync(path.join(folder, "docs"), { recursive: true });
  const notes = path.join(folder, "notes.md");
  writeFileSync(notes, "The password for the lighthouse is periwinkle.\nSecond line.\n");
  writeFileSync(path.join(folder, "docs", "readme.md"), "Docs.\n");
  const outside = path.join(ersa.dir, "dot.png");
  writeFileSync(outside, PNG);
  const outsideText = path.join(ersa.dir, "outside.txt");
  writeFileSync(outsideText, "Outside.\n");
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const added = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await added;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();
  // A folder without git runs in the project folder itself, with no Worktree checkbox, so its files are inside the
  // workspace.
  await expect(page.getByRole("checkbox", { name: "Worktree" })).toHaveCount(0);
  // The box takes attachments once the host's backends are detected (FLOW-NEW-SESSION-R23), which can end after the
  // project is added.
  const words = page.getByRole("textbox", { name: "Message" });
  await expect(words).toBeEnabled({ timeout: 60_000 });

  const attach = page.getByRole("button", { name: "Attach", exact: true });
  const choose = async (name: string, answer: string | string[]) => {
    await attach.click();
    const dialog = ersa.answerDialog(answer);
    await page.getByRole("menuitem", { name }).click();
    return dialog;
  };

  // R07: files, then images, each through the system's dialog; the same file twice is attached once (R12).
  const files = await choose("Add files…", [notes, notes, outsideText]);
  expect(files.options).toMatchObject({ directory: false, multiple: true });
  const images = await choose("Add images…", outside);
  expect(images.options).toMatchObject({ directory: false, filters: [{ extensions: expect.arrayContaining(["png"]) }] });
  const folders = await choose("Add a folder…", path.join(folder, "docs"));
  expect(folders.options).toMatchObject({ directory: true });

  const list = page.getByRole("list", { name: "Attachments" });
  await expect(list.getByRole("listitem")).toHaveCount(4);
  // R09, R17: each by name and detail; the file from outside the workspace says it will be copied, and the image,
  // which goes inline, does not.
  const copied = /Will be copied into the workspace's \.ersa folder/;
  await expect(list.getByRole("listitem").nth(0)).toHaveText(/notes\.md\s*2 lines/);
  await expect(list.getByRole("listitem").nth(0)).not.toHaveText(copied);
  await expect(list.getByRole("listitem").nth(1)).toHaveText(/outside\.txt\s*1 line.*Will be copied into the workspace's \.ersa folder/);
  await expect(list.getByRole("listitem").nth(2)).toHaveText(/dot\.png\s*\d+ bytes/);
  await expect(list.getByRole("listitem").nth(2)).not.toHaveText(copied);
  await expect(list.getByRole("listitem").nth(3)).toHaveText(/docs\s*Folder/);
  await ersa.shot("attached");

  // R13: the message goes with them, and the agent reads the file it was given.
  await words.fill("Read the attached notes.md and reply with only the password it names, in lowercase.");
  await words.press("Enter");
  const transcript = page.getByRole("region", { name: "Transcript" });
  await expect(transcript).toBeVisible({ timeout: 120_000 });
  await expect(transcript.getByText(/periwinkle/i).last()).toBeVisible({ timeout: 180_000 });
  await ersa.shot("answered");
});
