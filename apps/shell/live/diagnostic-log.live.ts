import { expect, test } from "./harness";

// Open diagnostic log in the running window (#571; FLOW-READ-RAW-LOG-R13). Every live check's window is a debug build
// attached to a labeled endpoint, where Ersa has no diagnostic log of its own: the button is in the developer pane's
// toolbar, unavailable, and says why, naming the label; choosing it opens nothing and says no problem. Opening the
// file itself needs a window that is not attached, which the harness never starts.

test("Open diagnostic log is unavailable in a window attached to a label, and says why", async ({ ersa }) => {
  const page = ersa.page;
  const rail = page.getByRole("navigation", { name: "Rail" });
  const arrange = rail.getByRole("button", { name: "Arrange sessions" });
  await expect(arrange).toBeEnabled({ timeout: 60_000 });
  await arrange.click();
  await page.getByRole("menuitemcheckbox", { name: "Developer pane" }).click();
  const pane = page.getByRole("complementary", { name: "Developer" });
  await expect(pane).toBeVisible();

  const button = pane.getByRole("button", { name: "Open diagnostic log" });
  const why = `This build is attached to ${ersa.label}, so it has no diagnostic log to open.`;
  await expect(button).toHaveAttribute("aria-disabled", "true");
  await expect(button).toHaveAttribute("title", why);
  await expect(button).toHaveAccessibleDescription(why);
  await button.hover();
  await ersa.shot("unavailable");

  // Choosing it anyway does nothing: no problem line, and it keeps keyboard focus.
  // Playwright counts aria-disabled as disabled, so the click is forced, as a person's click reaches it regardless.
  await button.click({ force: true });
  await button.focus();
  await expect(button).toBeFocused();
  await expect(pane.locator(".raw-log-problem[data-shown]")).toHaveCount(0);
  await ersa.shot("unavailable-focus");
});
