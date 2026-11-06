import { expect, test } from "@playwright/test";

// Every component section in the gallery, photographed in both themes. Each is
// photographed on a page showing it alone, so its baseline depends only on it.
const components = [
  "RailFoot",
  "QuitQuestion",
  "ProjectList",
  "AddProject",
  "SessionList",
  "Transcript",
  "SessionTitle",
  "ErrorBoundary",
  "NewSessionScreen",
  "Chip",
  "ContextRing",
  "Composer",
  "Icon",
  "Button",
];

for (const theme of ["light", "dark"]) {
  for (const component of components) {
    test(`${component} in ${theme}`, async ({ page }) => {
      await page.goto(`/gallery.html?component=${component}&theme=${theme}`);
      const section = page.locator(`[data-theme="${theme}"] [data-component="${component}"]`);
      await expect(section).toHaveScreenshot(`${component}-${theme}.png`);
    });
  }
}

// The screenshots above depend on this: a section photographed alone cannot be moved by its neighbours.
test("a filtered gallery shows one component in one theme", async ({ page }) => {
  await page.goto("/gallery.html?component=Chip&theme=dark");
  await expect(page.locator("[data-theme]")).toHaveCount(1);
  await expect(page.locator('[data-theme="dark"] [data-component="Chip"]')).toHaveCount(1);
  await expect(page.locator("[data-component]")).toHaveCount(1);
});

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

// DESIGN.md, Layout and Shapes: the user's message is a bubble at the right of the reading column, as wide as its
// words up to bubble-max, surface-raised with a line edge and radius-lg, padded space-md by space-lg, with no label.
test("the user's messages are bubbles, in both themes", async ({ page }) => {
  await page.goto("/gallery.html?component=Transcript");
  // A bubble's place and size in its reading column, whose content box sits inside the page's own padding.
  const measure = (el: Element) => {
    const s = getComputedStyle(el);
    const page = el.parentElement!;
    const ps = getComputedStyle(page);
    const box = el.getBoundingClientRect();
    const col = page.getBoundingClientRect();
    return {
      fill: s.backgroundColor,
      edge: s.borderTopColor,
      edgeWidth: s.borderTopWidth,
      radius: s.borderTopLeftRadius,
      padBlock: s.paddingTop,
      padInline: s.paddingLeft,
      right: col.right - parseFloat(ps.paddingRight) - box.right,
      share: box.width / (col.width - parseFloat(ps.paddingLeft) - parseFloat(ps.paddingRight)),
      overflow: el.scrollWidth - el.clientWidth,
      // Everything the bubble shows, which is its message and nothing else: no label.
      shown: (el as HTMLElement).innerText,
      words: el.querySelector(".transcript-text")?.textContent ?? "",
    };
  };
  for (const theme of ["light", "dark"]) {
    const transcript = page.locator(`[data-theme="${theme}"] [data-component="Transcript"]`);
    const twoTurns = transcript.locator("figure").filter({ hasText: "Two turns" });
    const bubbles = twoTurns.locator(".transcript-user");
    await expect(bubbles).toHaveCount(2);

    const tokens = await transcript.evaluate((el) => {
      const probe = document.createElement("div");
      probe.style.cssText =
        "background: var(--color-surface-raised); border: var(--line-hairline) solid var(--color-line);" +
        "border-radius: var(--radius-lg); padding: var(--space-md) var(--space-lg); max-width: var(--size-bubble-max);";
      el.appendChild(probe);
      const s = getComputedStyle(probe);
      const t = {
        fill: s.backgroundColor,
        edge: s.borderTopColor,
        edgeWidth: s.borderTopWidth,
        radius: s.borderTopLeftRadius,
        padBlock: s.paddingTop,
        padInline: s.paddingLeft,
        // bubble-max is a share of the column, so it computes to a percentage.
        max: parseFloat(s.maxWidth) / 100,
      };
      probe.remove();
      return t;
    });
    expect(tokens.max, "bubble-max is a share of the column").toBeGreaterThan(0);

    for (const bubble of await bubbles.all()) {
      const got = await bubble.evaluate(measure);
      expect(got.fill, theme).toBe(tokens.fill);
      expect(got.edge, theme).toBe(tokens.edge);
      expect(got.edgeWidth, theme).toBe(tokens.edgeWidth);
      expect(got.radius, theme).toBe(tokens.radius);
      expect(got.padBlock, theme).toBe(tokens.padBlock);
      expect(got.padInline, theme).toBe(tokens.padInline);
      expect(got.right, `${theme}: at the right of the column`).toBeCloseTo(0, 0);
      expect(got.share, `${theme}: no wider than bubble-max`).toBeLessThanOrEqual(tokens.max + 0.001);
      expect(got.shown.trim(), `${theme}: no label`).toBe(got.words.trim());
    }

    // A short message is as wide as its words, well short of bubble-max.
    const short = await bubbles.filter({ hasText: "Now delete" }).evaluate(measure);
    expect(short.share, theme).toBeLessThan(tokens.max);

    // A long word or path wraps inside the bubble rather than widening it.
    const long = await transcript.locator("figure").filter({ hasText: "Long path" }).locator(".transcript-user").evaluate(measure);
    expect(long.share, theme).toBeLessThanOrEqual(tokens.max + 0.001);
    expect(long.overflow, theme).toBeLessThanOrEqual(0);

    // Assistive technology still hears each message as the user's.
    await expect(twoTurns.getByRole("group", { name: "You" })).toHaveCount(2);
  }
});

// The gap between turns stays space-xxxl after the label is gone (DESIGN.md, Layout).
test("a new turn starts space-xxxl below the last", async ({ page }) => {
  await page.goto("/gallery.html?component=Transcript&theme=light");
  const twoTurns = page.locator('[data-component="Transcript"] figure').filter({ hasText: "Two turns" });
  const { gap, xxxl } = await twoTurns.locator(".transcript-user").nth(1).evaluate((el) => {
    const probe = document.createElement("div");
    probe.style.height = "var(--space-xxxl)";
    el.parentElement!.appendChild(probe);
    const xxxl = probe.getBoundingClientRect().height;
    probe.remove();
    const prev = el.previousElementSibling!;
    return { gap: el.getBoundingClientRect().top - prev.getBoundingClientRect().bottom, xxxl };
  });
  expect(gap).toBeCloseTo(xxxl, 0);
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
    for (const input of [".composer-input[data-force=focus]", ".session-heading-input[data-force=focus]"]) {
      const edge = await column.locator(input).first().evaluate((el) => getComputedStyle(el).borderTopColor);
      expect(edge, `${input} in ${theme}`).toBe(accent);
    }
  }
});

// The open list's shadow is soft enough to sit inside the screenshots' per-pixel threshold, so it is checked here.
// Its options take radius-md, so their corners nest within the list's (DESIGN.md, Shapes).
test("an open chip list floats: a shadow and the floating radius, its options nested, in both themes", async ({ page }) => {
  await page.goto("/gallery.html");
  for (const theme of ["light", "dark"]) {
    const list = page.locator(`[data-theme="${theme}"] [data-component="Chip"] .chip-list`).first();
    const { shadow, radius, floating, options, nested } = await list.evaluate((el) => {
      // Resolve a token to pixels by giving it to a probe inside the list.
      const resolve = (token: string) => {
        const probe = document.createElement("span");
        probe.style.borderRadius = `var(${token})`;
        el.appendChild(probe);
        const value = getComputedStyle(probe).borderTopLeftRadius;
        probe.remove();
        return value;
      };
      const style = getComputedStyle(el);
      return {
        shadow: style.boxShadow,
        radius: style.borderTopLeftRadius,
        floating: resolve("--radius-lg"),
        options: [...el.querySelectorAll(".chip-option")].map((o) => getComputedStyle(o).borderTopLeftRadius),
        nested: resolve("--radius-md"),
      };
    });
    expect(shadow, theme).not.toBe("none");
    expect(radius, theme).toBe(floating);
    expect(options.length, theme).toBeGreaterThan(0);
    for (const option of options) expect(option, `an option in ${theme}`).toBe(nested);
  }
});

// The rail's workspace headings hold their line before the names arrive, so the tiles never move when they do.
test("a workspace heading is as tall before its name is read as after", async ({ page }) => {
  await page.goto("/gallery.html");
  const headings = page.locator('[data-theme="light"] [data-component="SessionList"] .sessions-workspace-name');
  const heights = await headings.evaluateAll((els) => els.map((el) => el.getBoundingClientRect().height));
  // Two named headings, then the same two unnamed.
  expect(heights).toHaveLength(4);
  expect(heights[0]).toBeGreaterThan(0);
  expect(heights[2]).toBeCloseTo(heights[0] ?? 0, 1);
  expect(heights[3]).toBeCloseTo(heights[1] ?? 0, 1);
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

// The rail's foot (LAY-SHELL, The foot; issue #155): Retry attaches again, focus comes back to it when the retry
// fails, and reaching the sessions again is heard though nothing is shown.
test("Retry in the rail's foot attaches again, keeps keyboard focus, and says when it reconnects", async ({ page }) => {
  await page.goto("/gallery.html?component=RailFoot&theme=light");
  const foot = page.locator("figure").filter({ has: page.locator("figcaption", { hasText: /^Unreachable$/ }) });
  const retry = foot.getByRole("button", { name: "Retry" });
  const status = foot.getByRole("status");
  await expect(status).toHaveText("");

  // The first retry fails: Retry goes while Ersa connects, then comes back with focus.
  await retry.focus();
  await page.keyboard.press("Enter");
  await expect(retry).toHaveCount(0);
  await expect(retry).toBeFocused();

  // The next one reaches the sessions: the alert goes, and the status region says so out of sight.
  await page.keyboard.press("Enter");
  await expect(foot.getByRole("alert")).toHaveCount(0);
  await expect(status).toHaveText("Reconnected");
  const box = await status.boundingBox();
  expect((box?.width ?? 0) * (box?.height ?? 0)).toBeLessThanOrEqual(1);
});

test("Retry in the rail's foot leaves focus where the user put it while Ersa was connecting", async ({ page }) => {
  await page.goto("/gallery.html?component=RailFoot&theme=light");
  const foot = page.locator("figure").filter({ has: page.locator("figcaption", { hasText: /^Unreachable$/ }) });
  const retry = foot.getByRole("button", { name: "Retry" });
  await retry.focus();
  await page.keyboard.press("Enter");
  await expect(retry).toHaveCount(0);
  // Elsewhere in the window, such as the composer.
  await page.evaluate(() => {
    const input = document.createElement("input");
    input.id = "elsewhere";
    document.body.appendChild(input);
    input.focus();
  });
  await expect(retry).toBeVisible();
  await expect(page.locator("#elsewhere")).toBeFocused();
});

// RAIL.md: with reduced motion on, the foot's ring holds still and the foot appears without its fade.
test("the rail's foot holds still with reduced motion on", async ({ page }) => {
  const animations = async () => {
    await page.goto("/gallery.html?component=RailFoot&theme=light");
    const trying = page.locator("figure").filter({ has: page.locator("figcaption", { hasText: /^Reconnecting$/ }) });
    return trying.locator(".rail-foot").evaluate((el) => ({
      ring: getComputedStyle(el.querySelector(".rail-foot-trying")!, "::before").animationName,
      fade: getComputedStyle(el).animationName,
    }));
  };
  expect(await animations()).toEqual({ ring: "rail-foot-turn", fade: "rail-foot-in" });
  await page.emulateMedia({ reducedMotion: "reduce" });
  expect(await animations()).toEqual({ ring: "none", fade: "none" });
});
