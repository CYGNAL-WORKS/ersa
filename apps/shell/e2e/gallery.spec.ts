import { expect, type Locator, type Page, test } from "@playwright/test";

// Every component section in the gallery, photographed in both themes. Each is
// photographed on a page showing it alone, so its baseline depends only on it.
const components = [
  "Menu",
  "RailTop",
  "RailFoot",
  "QuitQuestion",
  "DeleteQuestion",
  "RailTree",
  "SessionsPage",
  "QuestionForm",
  "Shelf",
  "AttachmentChip",
  "AgentText",
  "Transcript",
  "ToolCard",
  "DiffCard",
  "ThinkingRow",
  "SessionTitle",
  "ErrorBoundary",
  "NewSessionScreen",
  "Chip",
  "ContextRing",
  "Workbench",
  "DeveloperPane",
  "SignInNote",
  "ArchivedNote",
  "Composer",
  "Icon",
  "Button",
];

// Each section is photographed in a window as tall as its page, so the page does not scroll (#390, #502, #532). In a
// page that scrolls, what is fixed in the window (an open list, a tooltip) is laid out and drawn one of two ways,
// depending on when the screenshot is taken: its text drawn differently, or a tooltip flipped above its chip. The
// page's height is read in the test's usual window once its fonts are in, and the page is then opened again in a
// window that tall: resizing a drawn page would set its tiles measuring again while the screenshot waits for them,
// and a window taller than the page would stretch the sections that fill it. A page taller than TALLEST keeps the
// usual window: a window that tall is slow to draw, and the rail's tree, some 30,000 pixels of breathing marks, then
// cannot hold still long enough for its screenshot.
const TALLEST = 8192;

for (const theme of ["light", "dark"]) {
  for (const component of components) {
    test(`${component} in ${theme}`, async ({ page }) => {
      // The shelf lies beside a rail-wide tile, wider than a theme's column at the default width.
      if (component === "Shelf") await page.setViewportSize({ width: 1600, height: 900 });
      const url = `/gallery.html?component=${component}&theme=${theme}`;
      await page.goto(url);
      await page.evaluate(async () => {
        await document.fonts.ready;
      });
      const { width, height } = page.viewportSize()!;
      const tall = await page.evaluate(() => document.documentElement.scrollHeight);
      if (tall > height && tall <= TALLEST) {
        await page.setViewportSize({ width, height: tall });
        await page.goto(url);
      }
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

// A suggestion's label says how far a lasting answer reaches, so on the shelf and on the transcript's card it wraps
// whole: nothing of it overflows its button, however narrow the surface, and no ellipsis cuts it.
// verifies: FLOW-APPROVE-PERMISSION-R30
test("a permission suggestion shows its whole label", async ({ page }) => {
  // The shelf lies beside a rail-wide tile, as in its screenshot.
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto("/gallery.html");
  for (const theme of ["light", "dark"]) {
    for (const component of ["Shelf", "Transcript"]) {
      const suggestions = page.locator(`[data-theme="${theme}"] [data-component="${component}"] .answers-suggestion`);
      expect(await suggestions.count(), `${component} in ${theme} shows suggestions`).toBeGreaterThan(0);
      const shelfWidth = await page.evaluate(() => {
        const probe = document.createElement("div");
        probe.style.width = "var(--size-shelf)";
        document.body.appendChild(probe);
        const width = probe.getBoundingClientRect().width;
        probe.remove();
        return width;
      });
      for (const suggestion of await suggestions.all()) {
        const got = await suggestion.evaluate((el) => {
          const s = getComputedStyle(el);
          return {
            label: el.textContent ?? "",
            width: el.getBoundingClientRect().width,
            overflowX: el.scrollWidth - el.clientWidth,
            overflowY: el.scrollHeight - el.clientHeight,
            ellipsis: s.textOverflow,
            wraps: s.whiteSpace !== "nowrap" && s.whiteSpace !== "pre",
          };
        });
        const where = `${component} in ${theme}: ${got.label}`;
        expect(got.overflowX, where).toBeLessThanOrEqual(0);
        expect(got.overflowY, where).toBeLessThanOrEqual(0);
        expect(got.ellipsis, where).not.toBe("ellipsis");
        expect(got.wraps, where).toBe(true);
        if (component === "Shelf") expect(got.width, where).toBeLessThanOrEqual(shelfWidth);
      }
    }
  }
  // The gallery's longest labels are longer than the shelf, so the check above saw them wrap rather than fit.
  const wrapped = await page
    .locator('[data-component="Shelf"] .answers-suggestion')
    .evaluateAll((els) => els.filter((el) => el.getBoundingClientRect().height > 2 * parseFloat(getComputedStyle(el).lineHeight)).length);
  expect(wrapped).toBeGreaterThan(0);
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
    for (const input of [".composer-box[data-force=focus]", ".session-heading-input[data-force=focus]"]) {
      const edge = await column.locator(input).first().evaluate((el) => getComputedStyle(el).borderTopColor);
      expect(edge, `${input} in ${theme}`).toBe(accent);
    }
    // The focused box holds the focal glow, unless a permission request holds it (docs/design/COMPOSER.md, Focus).
    const glow = (selector: string) => column.locator(selector).first().evaluate((el) => getComputedStyle(el).boxShadow);
    expect(await glow(".composer-box[data-force=focus]:not([data-quiet-focus])"), `the glow in ${theme}`).not.toBe("none");
    expect(await glow(".composer-box[data-force=focus][data-quiet-focus]"), `no glow in ${theme}`).toBe("none");
  }
});

// Send keeps its fill under the pointer and pressed, the pointer's material laid over it (docs/design/COMPOSER.md,
// The composer): a fill the browser drops would read as a missing button, which a screenshot's threshold may pass.
test("send keeps its primary fill under the pointer and pressed, in both themes", async ({ page }) => {
  await page.goto("/gallery.html");
  for (const theme of ["light", "dark"]) {
    for (const state of ["hover", "pressed"]) {
      const send = page.locator(`[data-theme="${theme}"] .composer-send[data-force="${state}"]`).first();
      const image = await send.evaluate((el) => getComputedStyle(el).backgroundImage);
      // Two layers: the pointer's material, then the primary fill beneath it.
      expect(image.match(/linear-gradient/g)?.length, `${state} in ${theme}: ${image}`).toBe(2);
    }
  }
});

// The context ring's arc is a hairline, so a band's colour sits inside the screenshots' per-pixel threshold; it is
// checked here, on its own and on the shelf's open tile (LAY-SHELL, Context): caution past half, danger past four fifths.
test("the context ring's arc takes its band's colour, in both themes", async ({ page }) => {
  await page.goto("/gallery.html");
  for (const theme of ["light", "dark"]) {
    const root = page.locator(`[data-theme="${theme}"]`).first();
    const token = (name: string) => root.evaluate((el, n) => {
      const probe = document.createElement("span");
      probe.style.color = `var(${n})`;
      el.appendChild(probe);
      const value = getComputedStyle(probe).color;
      probe.remove();
      return value;
    }, name);
    const stroke = (selector: string) => root.locator(`${selector} .context-ring-used`).first().evaluate((el) => getComputedStyle(el).stroke);
    const [muted, caution, danger] = [await token("--color-ink-muted"), await token("--color-caution"), await token("--color-danger")];
    expect(await stroke('[data-component="ContextRing"] .context-ring:not([data-band])'), `quiet in ${theme}`).toBe(muted);
    expect(await stroke('[data-component="ContextRing"] .context-ring[data-band="caution"]'), `caution in ${theme}`).toBe(caution);
    expect(await stroke('[data-component="ContextRing"] .context-ring[data-band="danger"]'), `danger in ${theme}`).toBe(danger);
    expect(await stroke('[data-component="Shelf"] .shelf-readouts .context-ring[data-band="caution"]'), `the shelf's in ${theme}`).toBe(caution);
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

/**
 * Opens the layer `opener` controls, with `opener` at the window's `edge`, and gives back the side the layer says it
 * opened on, its top as its entrance starts and as it ends, the opener's top, and how many animations it runs.
 */
async function entrance(opener: Locator, edge: "start" | "end") {
  return opener.evaluate(async (el, edge) => {
    el.scrollIntoView({ block: edge });
    // A specimen may be drawn open: close it first, so the entrance is this opening's.
    // React draws each click's outcome once its task is done; the entrance is paused at its start below, however far
    // it has run by then.
    const drawn = () => new Promise((resolve) => setTimeout(resolve));
    if (el.getAttribute("aria-expanded") === "true") {
      (el as HTMLElement).click();
      await drawn();
    }
    (el as HTMLElement).click();
    await drawn();
    const layer = document.getElementById(el.getAttribute("aria-controls") ?? "")!;
    const runs = layer.getAnimations();
    const top = () => layer.getBoundingClientRect().top;
    runs.forEach((a) => {
      a.pause();
      a.currentTime = 0;
    });
    const start = top();
    runs.forEach((a) => a.finish());
    return { side: layer.dataset.side, start, end: top(), opener: el.getBoundingClientRect().top, runs: runs.length };
  }, edge);
}

// A chip whose list the window may flip: the gallery holds the lists it shows open on one side.
const FLIPPABLE_CHIP = ".chip:not(:disabled):not(.gallery-room *):not(.gallery-list-above *)";

// COMPOSER.md, A chip's list and The attach menu (#374): a layer the window flips for want of room still slides
// toward its place away from what opened it, and with reduced motion it shows at once.
test("a flipped chip list or attach menu unfolds away from what opened it, and at once with reduced motion", async ({ page }) => {
  // A short window, so the layers flip.
  await page.setViewportSize({ width: 1280, height: 400 });
  await page.goto("/gallery.html?component=Composer&theme=light");
  // Attach at the window's top: no room above, so its menu opens below it and slides down.
  const below = await entrance(page.locator(".composer-attach:not(:disabled)").first(), "start");
  expect(below.side).toBe("below");
  expect(below.end).toBeGreaterThan(below.opener);
  expect(below.runs).toBeGreaterThan(0);
  expect(below.start).toBeLessThan(below.end);

  await page.goto("/gallery.html?component=Chip&theme=light");
  // A chip far enough down the page to sit at the window's foot: no room beneath, so its list opens above it and slides up.
  const above = await entrance(page.locator(FLIPPABLE_CHIP).last(), "end");
  expect(above.side).toBe("above");
  expect(above.end).toBeLessThan(above.opener);
  expect(above.runs).toBeGreaterThan(0);
  expect(above.start).toBeGreaterThan(above.end);

  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/gallery.html?component=Composer&theme=light");
  expect((await entrance(page.locator(".composer-attach:not(:disabled)").first(), "start")).runs).toBe(0);
  await page.goto("/gallery.html?component=Chip&theme=light");
  expect((await entrance(page.locator(FLIPPABLE_CHIP).last(), "end")).runs).toBe(0);
});

// A heading's actions from the keyboard (LAY-SHELL, A heading's actions): they show once focus enters the heading,
// More opens its menu with focus on the item, and Escape closes it with focus back on More.
test("a project heading's actions show with keyboard focus, and More's menu works from the keyboard", async ({ page }) => {
  await page.goto("/gallery.html?component=RailTree&theme=light");
  const tree = page.getByRole("figure", { name: "Projects", exact: true });
  // ersa's heading: the empty Pinned group before it is drawn closed.
  const heading = tree.locator('.rail-heading[data-slide="heading:p1"]');
  const actions = heading.locator(".rail-heading-actions");
  await expect(actions).toHaveCSS("opacity", "0");
  await heading.getByRole("button", { name: "ersa", exact: true }).focus();
  await page.keyboard.press("Tab");
  await expect(heading.getByRole("button", { name: "New session in ersa" })).toBeFocused();
  await expect(actions).toHaveCSS("opacity", "1");
  await page.keyboard.press("Tab");
  const more = heading.getByRole("button", { name: "More for ersa" });
  await expect(more).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(heading.getByRole("menuitem", { name: "Don't trust" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(heading.getByRole("menu")).toHaveCount(0);
  await expect(more).toBeFocused();

  // A press anywhere but on More or the menu closes it, New session's included.
  await more.click();
  await expect(heading.getByRole("menu")).toHaveCount(1);
  await heading.getByRole("button", { name: "New session in ersa" }).click();
  await expect(heading.getByRole("menu")).toHaveCount(0);
  await expect(more).toHaveAttribute("aria-expanded", "false");

  // Escape closes it while focus is anywhere in the heading.
  await more.click();
  await heading.getByRole("button", { name: "New session in ersa" }).focus();
  await page.keyboard.press("Escape");
  await expect(heading.getByRole("menu")).toHaveCount(0);
});

// What a heading's actions do (LAY-SHELL, A heading's actions): New session opens the screen for its own project,
// and More withdraws trust at once but asks before trusting (ADR 0028). The gallery keeps each call on the page.
test("a project heading's New session and More act on that project, and trusting asks first", async ({ page }) => {
  await page.goto("/gallery.html?component=RailTree&theme=light");
  const tree = page.getByRole("figure", { name: "Projects", exact: true });
  const call = () => page.evaluate(() => document.body.dataset.railCall);

  await tree.getByRole("button", { name: "New session in runner" }).click();
  expect(await call()).toBe("newSession p5");

  await tree.getByRole("button", { name: "More for ersa" }).click();
  // Opened with the pointer, focus is on the menu, not its item (RAIL.md, Menus).
  await expect(tree.getByRole("menu", { name: "More for ersa" })).toBeFocused();
  await tree.getByRole("menuitem", { name: "Don't trust" }).click();
  expect(await call()).toBe("setTrust p1 false");

  await tree.getByRole("button", { name: "More for stranger" }).click();
  await tree.getByRole("menuitem", { name: "Trust…" }).click();
  const question = tree.getByRole("region", { name: "Trust stranger?" });
  await expect(question).toBeVisible();
  expect(await call()).toBe("setTrust p1 false");
  await question.getByRole("button", { name: "Cancel" }).click();
  expect(await call()).toBe("setTrust p1 false");
  await expect(tree.getByRole("button", { name: "More for stranger" })).toBeFocused();

  await tree.getByRole("button", { name: "More for stranger" }).click();
  await tree.getByRole("menuitem", { name: "Trust…" }).click();
  await question.getByRole("button", { name: "Trust", exact: true }).click();
  expect(await call()).toBe("setTrust p4 true");
});

// More's menu floats above the rail (RAIL.md, Menus): at the bottom of the window it opens above More, in full
// view, and it takes no room in the rail, so nothing beneath it scrolls.
test("a project heading's menu at the window's foot opens above More, in view", async ({ page }) => {
  await page.goto("/gallery.html?component=RailTree&theme=light");
  const tree = page.getByRole("figure", { name: "Projects", exact: true });
  const more = tree.getByRole("button", { name: "More for stranger" });
  // The window ends just below More, so the menu has no room beneath it.
  const at = await more.boundingBox();
  await page.setViewportSize({ width: 1280, height: Math.ceil(at!.y + at!.height) + 4 });
  const heightBefore = await tree.evaluate((el) => el.scrollHeight);
  await more.click();
  const menu = tree.getByRole("menu", { name: "More for stranger" });
  await expect(menu).toBeVisible();
  const [m, b, viewport] = [await menu.boundingBox(), await more.boundingBox(), page.viewportSize()];
  expect(m && b && viewport).toBeTruthy();
  expect(m!.y + m!.height).toBeLessThanOrEqual(viewport!.height);
  expect(m!.y + m!.height).toBeLessThanOrEqual(b!.y);
  // Aligned to More's trailing edge.
  expect(Math.abs(m!.x + m!.width - (b!.x + b!.width))).toBeLessThan(1);
  expect(await tree.evaluate((el) => el.scrollHeight)).toBe(heightBefore);

  // It fades out as it closes, then goes.
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(more).toBeFocused();
});

// A heading's trailing words as it turns (RAIL.md, A project heading): expanded, "Not trusted" after the shield and
// no count; collapsed, the shield alone and the count, with "Not trusted" as a tooltip under the pointer.
test("a collapsed heading counts its sessions, and an untrusted one keeps its words in the shield's tooltip", async ({ page }) => {
  await page.goto("/gallery.html?component=RailTree&theme=light");
  const open = page.getByRole("figure", { name: "Projects", exact: true });
  await expect(open.locator('.rail-heading[data-slide="heading:p1"] .rail-heading-count')).toBeHidden();
  await expect(open.getByRole("button", { name: /^stranger/ })).toHaveAccessibleName("stranger Not trusted");

  const folded = page.getByRole("figure", { name: "Collapsed, not trusted", exact: true });
  const heading = folded.getByRole("button", { name: /^ersa/ });
  await expect(heading).toHaveAccessibleName(/^ersa Not trusted Needs you 3 sessions$/);
  await expect(folded.locator(".rail-heading-trust")).toBeHidden();
  await expect(folded.locator('.rail-heading[data-slide="heading:p1"] .rail-heading-count')).toBeVisible();
  const tip = folded.locator(".rail-heading-tip");
  await expect(tip).toBeHidden();
  await folded.locator(".rail-heading-shield").hover();
  await expect(tip).toBeVisible();
  await expect(tip).toHaveText("Not trusted");
});

// A tile's More and its mark (RAIL.md, A tile: More): a session that needs the user keeps its chevron, active or not,
// with More before it; while the title is edited, More is not drawn, so the mark stays in its square.
test("a tile keeps its chevron beside More, and its mark while its title is edited", async ({ page }) => {
  await page.goto("/gallery.html?component=RailTree&theme=light");
  const waiting = page.getByRole("figure", { name: "Active, needs you, pointer over it", exact: true });
  const item = waiting.locator(".session-item").filter({ hasText: "Fix the flaky reconnect test" });
  await expect(item.locator(".session-mark")).toHaveCSS("opacity", "1");
  await expect(item.locator(".session-chevron")).toBeVisible();
  await expect(item.locator(".session-more")).toHaveCSS("opacity", "1");
  const [more, mark] = [await item.locator(".session-more").boundingBox(), await item.locator(".session-mark").boundingBox()];
  expect(more!.x + more!.width).toBeLessThanOrEqual(mark!.x + 1);

  const renaming = page.getByRole("figure", { name: "Renaming on the tile", exact: true });
  const edited = renaming.locator(".session-item").filter({ has: page.getByRole("textbox", { name: "Session title" }) });
  await edited.getByRole("textbox", { name: "Session title" }).focus();
  // Read once any fade has had time to finish, so a mark fading out is not taken for one that stays.
  await page.waitForTimeout(500);
  expect(await edited.locator(".session-mark").evaluate((el) => getComputedStyle(el).opacity)).toBe("1");
  await expect(edited.locator(".session-more")).toHaveCount(0);
});

// The session controls from the keyboard (FLOW-NEW-SESSION-R01): every chip and the Worktree checkbox on the way
// from the composer, each chip opening its choices, Escape closing them with focus back on the chip, a choice taking
// effect, and Space toggling the checkbox with focus kept on it.
test("every session control is reachable and works from the keyboard", async ({ page }) => {
  await page.goto("/gallery.html");
  const screen = page
    .locator('[data-theme="light"] [data-component="NewSessionScreen"] figure')
    .filter({ hasText: "Ready, at the defaults" });
  const chips = screen.locator("button.chip");
  const names = await chips.evaluateAll((els) => els.map((el) => el.getAttribute("aria-label")));
  // Host and project, then backend, model, effort and permission mode: the backend shows with one (FLOW-NEW-SESSION-R48).
  expect(names.length).toBe(6);
  const worktree = screen.getByRole("checkbox", { name: "Worktree" });

  // Tab from the composer reaches send, which keeps focus with nothing to send, then each control in turn: host,
  // project and Worktree where the session runs, then how it runs.
  await screen.locator("textarea").focus();
  await page.keyboard.press("Tab");
  expect(await page.evaluate(() => document.activeElement?.getAttribute("aria-label"))).toBe("Send");
  const reached: (string | null)[] = [];
  for (let i = 0; i <= names.length; i++) {
    await page.keyboard.press("Tab");
    reached.push(await page.evaluate(() => document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.closest("label")?.textContent ?? null));
  }
  expect(reached).toEqual([names[0], names[1], "Worktree", ...names.slice(2)]);

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
  const mode = screen.getByRole("button", { name: /^Permission mode:/ });
  await mode.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(screen.getByRole("button", { name: "Permission mode: Accept edits" })).toBeFocused();

  // Space unchecks the Worktree box and checks it again, with keyboard focus on it all the while; the focus edge is
  // ink (DESIGN.md, Pointer and press).
  await expect(worktree).toBeChecked();
  await worktree.focus();
  await page.keyboard.press("Space");
  await expect(worktree).not.toBeChecked();
  await expect(worktree).toBeFocused();
  const ink = await screen.locator(".worktree").evaluate((el) => {
    const probe = el.appendChild(document.createElement("span"));
    probe.style.color = "var(--color-ink)";
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  });
  // Polled, since the edge eases in over motion-quick.
  await expect.poll(() => screen.locator(".worktree").evaluate((el) => getComputedStyle(el).borderTopColor)).toBe(ink);
  await page.keyboard.press("Space");
  await expect(worktree).toBeChecked();
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

// A slide's transform would make the tile the shelf's containing block and take the shelf off its anchor, so a tile
// holding a shelf slides by its offset, and the shelf goes with it.
test("a shelf stays beside its tile while the tile slides", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto("/gallery.html?component=Shelf&theme=light");
  const gaps = await page.evaluate(async () => {
    const tile = document.querySelector<HTMLElement>(".session-item[data-slide-top]")!;
    const shelf = tile.querySelector(".shelf")!;
    const gap = () => shelf.getBoundingClientRect().top - tile.getBoundingClientRect().top;
    const moved = () => tile.getBoundingClientRect().top;
    const before = { gap: gap(), top: moved() };
    const slide = tile.animate([{ top: "24px" }, { top: "0px" }], { duration: 100000 });
    slide.pause();
    await new Promise(requestAnimationFrame);
    await new Promise(requestAnimationFrame);
    return { before, during: { gap: gap(), top: moved() } };
  });
  expect(gaps.during.top).toBeCloseTo(gaps.before.top + 24, 0);
  expect(gaps.during.gap).toBeCloseTo(gaps.before.gap, 0);
});

// DESIGN.md, Motion: a working session's glyph breathes its tone on a motion-breath loop; a chevron that comes to need
// the user breathes once over motion-breathe-once; both with motion-ease-in-out. With reduced motion on, neither moves.
test("a working glyph breathes, a new chevron breathes once, and both hold still with reduced motion on", async ({ page }) => {
  const animations = async () => {
    await page.goto("/gallery.html?component=RailTree&theme=light");
    return page.evaluate(() => {
      const glyph = document.querySelector('.session-glyph[data-breath="loop"]')!;
      // The rail marks a chevron to breathe only as its session comes to need the user, so it is marked here.
      const button = document.querySelector(".session-chevron-button")!;
      button.setAttribute("data-breath", "once");
      // The active tile's chevron is a mark, not a button; the gallery's tiles have none, so one is added.
      const mark = document.createElement("span");
      mark.className = "session-chevron";
      mark.dataset.breath = "once";
      button.parentElement!.append(mark);
      const of = (el: Element) => {
        const style = getComputedStyle(el);
        return [style.animationName, style.animationDuration, style.animationTimingFunction, style.animationIterationCount].join(" ");
      };
      return { glyph: of(glyph), chevron: of(button.firstElementChild!), mark: of(mark) };
    });
  };
  expect(await animations()).toEqual({
    glyph: "session-breath 3s cubic-bezier(0.4, 0, 0.2, 1) infinite",
    chevron: "session-breath 2.4s cubic-bezier(0.4, 0, 0.2, 1) 1",
    mark: "session-breath 2.4s cubic-bezier(0.4, 0, 0.2, 1) 1",
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
  const still = await animations();
  expect([still.glyph.split(" ")[0], still.chevron.split(" ")[0], still.mark.split(" ")[0]]).toEqual(["none", "none", "none"]);
});

// A project heading (docs/design/RAIL.md): the note after a withdrawal of trust starts where the project's name does,
// and an expanded, trusted heading keeps no empty place toward its trailing edge that would cut its name shorter.
test("a heading's trust note starts at its name, and an expanded trusted heading has no empty status, in both themes", async ({ page }) => {
  await page.goto("/gallery.html");
  const figure = (theme: string, label: string) =>
    page.locator(`[data-theme="${theme}"] [data-component="RailTree"] figure`).filter({ has: page.locator("figcaption", { hasText: new RegExp(`^${label}$`) }) });
  // The project's own heading, past the Pinned group's, which is drawn closed.
  const statusShown = (theme: string, label: string) =>
    figure(theme, label).locator(".rail-project:not(.rail-pinned *) .rail-heading-status").first().evaluate((el) => getComputedStyle(el).display !== "none");
  for (const theme of ["light", "dark"]) {
    for (const label of ["Trust withdrawn", "Trust withdrawn, collapsed"]) {
      const withdrawn = figure(theme, label).locator(".rail-project:has(.rail-project-note[data-open])");
      const name = await withdrawn.locator(".rail-heading-name").first().evaluate((el) => el.getBoundingClientRect().left);
      const text = await withdrawn.locator(".rail-project-note-text").first().evaluate((el) => {
        const range = document.createRange();
        range.selectNodeContents(el);
        return range.getBoundingClientRect().left;
      });
      expect(Math.abs(text - name), `${label} in ${theme}`).toBeLessThan(1);
      // Not trusted, its status holds "Not trusted".
      expect(await statusShown(theme, label), `${label} in ${theme}`).toBe(true);
    }
    expect(await statusShown(theme, "Saving trust"), `expanded and trusted in ${theme}`).toBe(false);
    expect(await statusShown(theme, "Collapsed"), `collapsed in ${theme}`).toBe(true);
  }
});

// The sign-in note on the new-session screen (docs/design/COMPOSER.md, The sign-in note), in a stage's size: with
// room, the box and the session controls keep their place whether the note shows or not and however tall its words
// are, and the wordmark stays space-xl above the note; in Ersa's smallest window (640 by 400, less the rail) the
// screen scrolls, so the session controls stay in reach.
const RAIL = 288;
/** A NewSessionScreen scene's screen, its specimen given a stage's size and clipped to it, as the window's stage is. */
async function screenIn(page: Page, scene: string, width: number, height: number) {
  const figure = page.locator('[data-theme="light"] [data-component="NewSessionScreen"] figure').filter({
    has: page.getByText(scene, { exact: true }),
  });
  const section = figure.locator(".new-session");
  await section.evaluate(
    (el, size) => {
      const stage = el.parentElement as HTMLElement;
      Object.assign(stage.style, { display: "flex", flexDirection: "column", width: `${size.width}px`, height: `${size.height}px`, overflow: "hidden" });
    },
    { width, height },
  );
  await settled(section);
  return section;
}

/**
 * Waits until nothing in the screen is still easing or arriving: the fonts loaded and two frames first, so what a
 * resize or a change sets off has started, then until every animation with an end has ended.
 */
async function settled(section: Locator) {
  // The fonts first: the box's place follows the composer's height, which the fonts set.
  await section.evaluate(() => document.fonts.ready);
  await section.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
  await expect
    .poll(() => section.evaluate((el) => el.getAnimations({ subtree: true }).filter((a) => a.effect?.getTiming().iterations !== Infinity).length))
    .toBe(0);
}

test("the sign-in note leaves the box and the session controls in place, and the wordmark clear above it", async ({ page }) => {
  await page.goto("/gallery.html?component=NewSessionScreen&theme=light");
  const scenes = ["Ready, at the defaults", "Backend signed out, the only one", "Signing in, waiting in the browser", "Sign-in didn't finish"];
  const places: number[] = [];
  for (const scene of scenes) {
    const section = await screenIn(page, scene, 800, 600);
    const place = await section.evaluate((el) => {
      const top = el.getBoundingClientRect().top;
      const box = el.querySelector(".composer-box")!.getBoundingClientRect().top - top;
      const controls = el.querySelector(".session-controls")!.getBoundingClientRect().top - top;
      const note = el.querySelector(".sign-in-note")?.getBoundingClientRect();
      const wordmark = el.querySelector(".new-session-wordmark")!.getBoundingClientRect();
      const probe = el.appendChild(document.createElement("div"));
      probe.style.height = "var(--space-xl)";
      const xl = probe.getBoundingClientRect().height;
      probe.remove();
      return { box, controls, clear: note ? note.top - wordmark.bottom >= xl : true };
    });
    expect(place.clear, `${scene}: the wordmark is space-xl above the note`).toBe(true);
    places.push(place.box, place.controls);
  }
  // Each scene's box and controls where the screen without a note has them.
  for (let i = 2; i < places.length; i += 2) {
    expect([places[i], places[i + 1]]).toEqual([places[0], places[1]]);
  }
});

test("in Ersa's smallest window the screen with the sign-in note scrolls, so the session controls stay in reach", async ({ page }) => {
  await page.goto("/gallery.html?component=NewSessionScreen&theme=light");
  // A note whose words run long, with the status line beneath it, and no trust question, which would take its own
  // place in the column whatever the note does: what stands above the box is taller than the window has room for.
  const section = await screenIn(page, "Signed out, beside a backend that isn't working", 640 - RAIL, 400);
  await section.evaluate((el) => {
    const words = el.querySelector(".sign-in-words") as HTMLElement;
    words.style.minHeight = "60vh";
  });
  await settled(section);
  const reach = await section.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
    const screen = (el.parentElement as HTMLElement).getBoundingClientRect();
    const controls = el.querySelector(".session-controls")!.getBoundingClientRect();
    return { scrolls: el.scrollHeight > el.clientHeight, inside: controls.top >= screen.top && controls.bottom <= screen.bottom };
  });
  expect(reach).toEqual({ scrolls: true, inside: true });
});

test("the room the sign-in note takes eases to each new height over motion-settle, and at once with reduced motion", async ({ page }) => {
  for (const reducedMotion of ["no-preference", "reduce"] as const) {
    await page.emulateMedia({ reducedMotion });
    await page.goto("/gallery.html?component=NewSessionScreen&theme=light");
    const section = await screenIn(page, "Signing in, waiting in the browser", 800, 600);
    // The note's words grow by a line, as a failure's reason would: the room eases to the new height, the wordmark
    // riding up with it, and the box stays where it was.
    const eased = await section.evaluate((el) => {
      const zone = el.querySelector(".new-session-above") as HTMLElement;
      // The box's place within its own screen, which is what keeps still; the page around the scene may move.
      const box = () => el.querySelector(".composer-box")!.getBoundingClientRect().top - el.getBoundingClientRect().top;
      const before = box();
      const line = () => {
        const p = document.createElement("p");
        p.textContent = "A line more beneath the note.";
        p.style.margin = "0";
        return p;
      };
      el.querySelector(".new-session-above-content")!.append(line());
      // The gallery stacks its scenes in one page, and a scene above this one may be easing its own note's room
      // meanwhile, as the narrower scenes above do once the fonts have loaded and rewrapped their words (#655), which
      // moves this scene in the page: one is made to, every time, so only a read within the screen passes.
      const above = [...document.querySelectorAll('[data-theme="light"] [data-component="NewSessionScreen"] figure')].find(
        (f) => f.querySelector("figcaption")?.textContent?.trim() === "Backend signed out, the only one",
      );
      above!.querySelector(".new-session-above-content")!.append(line());
      return new Promise<{ eases: number; timing?: { duration: unknown; easing: unknown }; settle: string; curve: string; boxMoved: number }>((resolve) =>
        // Two frames: the room is told of the new height after the first frame's layout.
        requestAnimationFrame(() => requestAnimationFrame(() => {
          const eases = zone.getAnimations();
          const style = getComputedStyle(zone);
          const timing = eases[0]?.effect?.getTiming();
          resolve({
            eases: eases.length,
            timing: timing && { duration: timing.duration, easing: timing.easing },
            settle: style.getPropertyValue("--motion-settle").trim(),
            curve: style.getPropertyValue("--motion-ease-out").trim(),
            boxMoved: box() - before,
          });
        })),
      );
    });
    expect(eased.boxMoved).toBe(0);
    if (reducedMotion === "reduce") {
      expect(eased.eases).toBe(0);
    } else {
      expect(eased.eases).toBe(1);
      expect(eased.timing).toEqual({ duration: parseFloat(eased.settle), easing: eased.curve });
    }
  }
});
