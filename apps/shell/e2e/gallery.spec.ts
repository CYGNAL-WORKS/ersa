import { expect, test } from "@playwright/test";

// Every component section in the gallery, photographed in both themes. Each is
// photographed on a page showing it alone, so its baseline depends only on it.
const components = [
  "Menu",
  "RailTop",
  "RailFoot",
  "QuitQuestion",
  "RailTree",
  "Shelf",
  "AttachmentChip",
  "Transcript",
  "SessionTitle",
  "ErrorBoundary",
  "NewSessionScreen",
  "Chip",
  "ContextRing",
  "ArchivedNote",
  "Composer",
  "Icon",
  "Button",
];

for (const theme of ["light", "dark"]) {
  for (const component of components) {
    test(`${component} in ${theme}`, async ({ page }) => {
      // The shelf lies beside a rail-wide tile, wider than a theme's column at the default width.
      if (component === "Shelf") await page.setViewportSize({ width: 1600, height: 900 });
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

// A heading's actions from the keyboard (LAY-SHELL, A heading's actions): they show once focus enters the heading,
// More opens its menu with focus on the item, and Escape closes it with focus back on More.
test("a project heading's actions show with keyboard focus, and More's menu works from the keyboard", async ({ page }) => {
  await page.goto("/gallery.html?component=RailTree&theme=light");
  const tree = page.getByRole("figure", { name: "Projects", exact: true });
  // ersa's heading: the empty Pinned group before it is drawn closed.
  const heading = tree.locator('.rail-heading[data-slide="project:p1"]');
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
  await expect(open.locator('.rail-heading[data-slide="project:p1"] .rail-heading-count')).toBeHidden();
  await expect(open.getByRole("button", { name: /^stranger/ })).toHaveAccessibleName("stranger Not trusted");

  const folded = page.getByRole("figure", { name: "Collapsed, not trusted", exact: true });
  const heading = folded.getByRole("button", { name: /^ersa/ });
  await expect(heading).toHaveAccessibleName(/^ersa Not trusted Needs you 3 sessions$/);
  await expect(folded.locator(".rail-heading-trust")).toBeHidden();
  await expect(folded.locator('.rail-heading[data-slide="project:p1"] .rail-heading-count')).toBeVisible();
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

  // Tab from the composer reaches send, which keeps focus with nothing to send, then each chip in turn.
  await screen.locator("textarea").focus();
  await page.keyboard.press("Tab");
  expect(await page.evaluate(() => document.activeElement?.getAttribute("aria-label"))).toBe("Send");
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
    // Focus starts in the list's search box where it has one, the workspace list's base branch, else on the chosen option.
    const search = list.getByRole("searchbox");
    if ((await search.count()) > 0) await expect(search).toBeFocused();
    else await expect(list.locator('[aria-selected="true"]')).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(list).toHaveCount(0);
    await expect(chip).toBeFocused();
    await expect(chip).toHaveAttribute("aria-label", name ?? "");
  }

  // A choice from the keyboard changes the value and leaves focus on the chip.
  // The workspace list opens in its search box; the option above it is Project folder.
  const workspace = screen.getByRole("button", { name: "Workspace: New workspace from main" });
  await workspace.focus();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  const folder = screen.getByRole("button", { name: "Workspace: Project folder" });
  await expect(folder).toBeFocused();
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
