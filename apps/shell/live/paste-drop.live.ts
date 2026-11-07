import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Paste and drop in the running window (#192; FLOW-SEND-MESSAGE-R08): an image pasted into the words attaches inline,
// files and a folder dropped on the box attach with their details, and the agent gets them.
test.use({ backend: "claude" });

// A 1 by 1 PNG.
const PNG = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

test("a pasted image and files dropped on the box attach, and the agent reads them", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "paste-drop-check");
  mkdirSync(path.join(folder, "docs"), { recursive: true });
  const notes = path.join(folder, "notes.md");
  writeFileSync(notes, "The lighthouse keeper's cat is called Marmalade.\n");
  writeFileSync(path.join(folder, "docs", "readme.md"), "Docs.\n");
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const added = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await added;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();
  await page.getByRole("button", { name: /^Workspace:/ }).click();
  await page.getByRole("option", { name: /Project folder/ }).click();

  // Paste an image into the words, as the clipboard holds one after a screenshot.
  const words = page.getByRole("textbox", { name: "Message" });
  await words.focus();
  await words.evaluate((el, png) => {
    const bytes = Uint8Array.from(atob(png), (c) => c.charCodeAt(0));
    const data = new DataTransfer();
    data.items.add(new File([bytes], "image.png", { type: "image/png" }));
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, PNG);
  const list = page.getByRole("list", { name: "Attachments" });
  await expect(list.getByRole("listitem")).toHaveCount(1);
  await expect(list.getByRole("listitem").nth(0)).toHaveText(/Pasted image\s*\d+ bytes/);

  // Drop a file and a folder on the box, as from the file manager.
  const box = page.getByRole("form", { name: "Composer" });
  await ersa.drop([notes, path.join(folder, "docs")], box);
  await expect(list.getByRole("listitem")).toHaveCount(3);
  await expect(list.getByRole("listitem").nth(1)).toHaveText(/notes\.md\s*1 line/);
  await expect(list.getByRole("listitem").nth(2)).toHaveText(/docs\s*Folder/);
  // Dropped off the box, nothing more is attached.
  await ersa.drop([path.join(folder, "docs", "readme.md")]);
  await expect(list.getByRole("listitem")).toHaveCount(3);
  await ersa.shot("pasted-and-dropped");

  await words.fill("Read the attached notes.md and reply with only the cat's name, spelled backwards in lowercase.");
  await words.press("Enter");
  const transcript = page.getByRole("region", { name: "Transcript" });
  await expect(transcript).toBeVisible({ timeout: 120_000 });
  // Spelled backwards, so the answer is the agent's, not the file's text shown in a tool card. No screenshot: the
  // transcript shows the test folder's full path.
  await expect(transcript.getByText(/edalamram/i).last()).toBeVisible({ timeout: 180_000 });
});
