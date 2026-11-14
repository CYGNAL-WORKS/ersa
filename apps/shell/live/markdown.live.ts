import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test } from "./harness";

// The agent's words as formatted text in the running window (#320): a real Claude Code reply with a table, a list,
// a link and a code block renders as those elements, and Copy puts the block's code on the clipboard.
test.use({ backend: "claude" });

const reply = [
  "## Result",
  "",
  "| Step | State |",
  "| --- | --- |",
  "| Build | done |",
  "",
  "- first item",
  "- second item",
  "",
  "See [the docs](https://example.com/docs).",
  "",
  "```bash",
  "pnpm test --run",
  "```",
].join("\n");

test("a reply with a table, a list, a link and a code block renders as formatted text, and Copy copies the code", async ({ ersa }) => {
  test.setTimeout(300_000);
  const { page } = ersa;
  const folder = path.join(ersa.dir, "markdown-check");
  mkdirSync(folder);
  const rail = page.getByRole("navigation", { name: "Rail" });

  await page.getByRole("button", { name: "Project: Choose a folder" }).click();
  const answered = ersa.answerDialog(folder);
  await page.getByRole("option", { name: /Add project…/ }).click();
  await answered;
  await page.getByRole("button", { name: "Trust", exact: true }).click();
  await expect(rail.getByRole("button", { name: /^New session in / })).toBeVisible();

  await rail.getByRole("button", { name: /^New session in / }).click();
  await page
    .getByRole("textbox", { name: "Message" })
    .fill(`Reply with exactly the Markdown between the lines, character for character, and nothing else. Use no tools.\n-----\n${reply}\n-----`);
  await page.keyboard.press("Enter");

  const text = page.locator(".agent-text").last();
  await expect(text.locator("pre code")).toHaveText("pnpm test --run", { timeout: 180_000 });
  await expect(text.locator("table th")).toHaveText(["Step", "State"]);
  await expect(text.locator("table td")).toHaveText(["Build", "done"]);
  await expect(text.locator("li")).toHaveText(["first item", "second item"]);
  const link = text.getByRole("link", { name: "the docs" });
  await expect(link).toHaveAttribute("href", "https://example.com/docs");
  await expect(text.locator(".code-block-language")).toHaveText("bash");
  await ersa.shot("formatted-light");

  // Copy: the code alone reaches the clipboard, and the button says so.
  const copy = text.getByRole("button", { name: "Copy code" });
  await copy.click();
  await expect(text.getByRole("button", { name: "Copied" })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("pnpm test --run");
  await ersa.shot("copied-light");

  await page.mouse.move(0, 0);
  await page.emulateMedia({ colorScheme: "dark" });
  await page.waitForTimeout(300);
  await ersa.shot("formatted-dark");
  await page.emulateMedia({ colorScheme: "light" });
});
