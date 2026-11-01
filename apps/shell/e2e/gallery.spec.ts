import { expect, test } from "@playwright/test";

// Every component section in the gallery, photographed in both themes.
const components = ["CoreStatus", "ProjectList", "Button"];

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
