import { expect, test } from "@playwright/test";

// Every component section in the gallery, photographed in both themes.
const components = [
  "CoreStatus",
  "ProjectList",
  "AddProject",
  "SessionList",
  "Transcript",
  "ErrorBoundary",
  "NewSessionScreen",
  "Chip",
  "ContextRing",
  "Composer",
  "Button",
];

for (const theme of ["light", "dark"]) {
  for (const component of components) {
    test(`${component} in ${theme}`, async ({ page }) => {
      await page.goto("/gallery.html");
      const section = page.locator(`[data-theme="${theme}"] [data-component="${component}"]`);
      await expect(section).toHaveScreenshot(`${component}-${theme}.png`);
    });
  }
}

test("the gallery shows every component", async ({ page }) => {
  await page.goto("/gallery.html");
  const shown = await page.locator('[data-theme="light"] [data-component]').evaluateAll((els) =>
    els.map((el) => el.getAttribute("data-component")),
  );
  expect(shown).toEqual(components);
});

// The safety surface: nothing in an open permission request may sit below a scrollbar.
test("an open permission request shows its fields whole", async ({ page }) => {
  await page.goto("/gallery.html");
  const fields = page.locator('[data-component="Transcript"] .transcript-permission[data-open] .transcript-field dd');
  expect(await fields.count()).toBeGreaterThan(0);
  const clipped = await fields.evaluateAll((els) =>
    els.filter((el) => {
      const style = getComputedStyle(el);
      return style.maxHeight !== "none" || style.overflowY !== "visible";
    }).length,
  );
  expect(clipped).toBe(0);
});

test("a focused input's edge is the accent, in both themes", async ({ page }) => {
  await page.goto("/gallery.html");
  for (const theme of ["light", "dark"]) {
    const column = page.locator(`[data-theme="${theme}"]`);
    const accent = await column.evaluate((el) => {
      const probe = document.createElement("span");
      probe.style.color = "var(--color-accent)";
      el.appendChild(probe);
      const color = getComputedStyle(probe).color;
      probe.remove();
      return color;
    });
    for (const input of [".add-project-input[data-force=focus]", ".composer-input[data-force=focus]"]) {
      const edge = await column.locator(input).first().evaluate((el) => getComputedStyle(el).borderTopColor);
      expect(edge, `${input} in ${theme}`).toBe(accent);
    }
  }
});

// The session controls from the keyboard (FLOW-NEW-SESSION-R01): every chip on the way from the composer, each
// opening its choices, Escape closing them with focus back on the chip, and a choice taking effect.
test("every session control is reachable and works from the keyboard", async ({ page }) => {
  await page.goto("/gallery.html");
  const screen = page
    .locator('[data-theme="light"] [data-component="NewSessionScreen"] figure')
    .filter({ hasText: "Ready, at the defaults" });
  const chips = screen.locator(".chip");
  const names = await chips.evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")));
  expect(names.length).toBe(6);

  // Tab from the composer reaches each chip in turn.
  await screen.locator("textarea").focus();
  const reached: (string | null)[] = [];
  for (let i = 0; i < names.length; i++) {
    await page.keyboard.press("Tab");
    reached.push(await page.evaluate(() => document.activeElement?.getAttribute("aria-label") ?? null));
  }
  expect(reached).toEqual(names);

  for (const name of names) {
    const chip = screen.getByRole("button", { name: name ?? "" });
    await chip.focus();
    await page.keyboard.press("Enter");
    const list = screen.getByRole("listbox");
    await expect(list).toBeVisible();
    // Focus starts on the chosen option.
    await expect(list.locator('[aria-selected="true"]')).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(list).toHaveCount(0);
    await expect(chip).toBeFocused();
    await expect(chip).toHaveAttribute("aria-label", name ?? "");
  }

  // A choice from the keyboard changes the value and leaves focus on the chip.
  const workspace = screen.getByRole("button", { name: "Workspace: New workspace" });
  await workspace.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  const root = screen.getByRole("button", { name: "Workspace: Root workspace" });
  await expect(root).toBeFocused();
});

// verifies: FLOW-NEW-SESSION-R10
test("with no project, the new-session screen asks whether the user trusts a folder before adding it", async ({ page }) => {
  await page.goto("/gallery.html");
  const screen = page
    .locator('[data-theme="light"] [data-component="NewSessionScreen"] figure')
    .filter({ has: page.locator("figcaption", { hasText: /^No project$/ }) });
  const folder = screen.getByRole("textbox", { name: "Project folder" });
  await folder.fill("/home/me/clones/stranger");
  await folder.press("Enter");
  const question = screen.getByRole("region", { name: "Trust stranger?" });
  await expect(question).toBeVisible();
  await expect(question.getByRole("button", { name: "Add, trusted" })).toBeVisible();
  await expect(question.getByRole("button", { name: "Add, not trusted" })).toBeVisible();
});
