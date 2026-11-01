import { expect, test } from "@playwright/test";

// Every component section in the gallery, photographed in both themes.
const components = ["CoreStatus", "ProjectList", "AddProject", "SessionList", "Transcript", "ErrorBoundary", "Composer", "Button"];

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
