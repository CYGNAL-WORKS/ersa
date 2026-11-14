import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// Tool cards and the thinking row in the running window (#327): a real Claude Code turn's tool calls show as cards
// that open and close, a subagent's call sits inside the card of the call that started it, a path in the workspace
// shows relative to it, and the agent's thinking shows as a row. Read-only tools only, so no permission is asked.
test.use({ backend: "claude" });

test("a turn's tool calls show as cards that open and close, with a subagent's call inside its card, and a thinking row", async ({ ersa }) => {
  test.setTimeout(420_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "tools-check");
  mkdirSync(path.join(folder, "docs"), { recursive: true });
  writeFileSync(path.join(folder, "docs", "notes.txt"), "The tool card check reads this line.\n");
  const rail = page.getByRole("navigation", { name: "Rail" });
  // The screenshots are published: the temporary folder's path names the owner's account, so before each one the
  // page's text says "<temp>" in its place. Nothing asserted reads the page after it.
  const shot = async (name: string) => {
    await page.evaluate((dir) => {
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      const forms = [dir, dir.replace(/\\/g, "/")];
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        for (const form of forms) if (node.nodeValue?.includes(form)) node.nodeValue = node.nodeValue.split(form).join("<temp>");
      }
    }, ersa.dir);
    await ersa.shot(name);
  };

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  await rail.getByRole("button", { name: /^New session in / }).click();
  await page
    .getByRole("textbox", { name: "Message" })
    .fill(
      [
        "Think it through first. Then, in this order:",
        "1. Use the Read tool on docs/notes.txt in this folder.",
        "2. Use the Agent tool (subagent type general-purpose) with the description Count files, asking it to use the Glob tool with the pattern **/* and report how many files it found.",
        "Use no other tools, then reply with one short sentence.",
      ].join("\n"),
    );
  await page.keyboard.press("Enter");

  const transcript = page.getByRole("region", { name: "Transcript" });
  // The Read card: its path relative to the project folder, done, closed at first.
  const read = transcript.getByRole("button", { name: /^Read docs[\\/]notes\.txt, Done$/ });
  await expect(read).toBeVisible({ timeout: 240_000 });
  await expect(read).toHaveAttribute("aria-expanded", "false");
  await expect(read.locator(".tool-detail")).toHaveText(/^(\d+s)?$/);

  // The Agent card, with its Glob call inside it and not as a block of the turn.
  const agent = transcript.getByRole("button", { name: /^Agent Count files, Done$/ });
  await expect(agent).toBeVisible({ timeout: 300_000 });
  await expect(transcript.locator(".transcript-page > .tool-card .tool-calls .tool-row[aria-label^='Glob']")).toHaveCount(1);
  await expect(transcript.locator(".transcript-page > .tool-card > .tool-row[aria-label^='Glob']")).toHaveCount(0);

  // The thinking row.
  await expect(transcript.locator(".thinking-row").first()).toHaveText(/^(Thinking…|Thought)$/);
  await expect(transcript.getByText("Thought").first()).toBeVisible({ timeout: 60_000 });
  await shot("cards-closed-light");

  // Open the Read card by keyboard: it shows its input and its output; it closes again.
  await read.focus();
  await page.keyboard.press("Enter");
  await expect(read).toHaveAttribute("aria-expanded", "true");
  const readCard = read.locator("xpath=..");
  await expect(readCard.locator(".tool-output")).toContainText("The tool card check reads this line.");
  await expect(readCard.locator(".tool-label")).toHaveText(["Input", "Output"]);
  // Open the Agent card by pointer, with its subagent's call inside.
  await agent.click();
  await expect(agent).toHaveAttribute("aria-expanded", "true");
  await expect(agent.locator("xpath=..").getByRole("button", { name: /^Glob .*, Done$/ })).toBeVisible();
  await page.mouse.move(0, 0);
  await page.waitForTimeout(500);
  await shot("cards-open-light");

  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForTimeout(300);
  await shot("cards-open-dark");
  await page.emulateMedia({ colorScheme: "light" });

  await read.click();
  await expect(read).toHaveAttribute("aria-expanded", "false");
  await agent.click();
  await expect(agent).toHaveAttribute("aria-expanded", "false");
  await page.waitForTimeout(500);
  await shot("cards-closed-again-light");
});
