// @vitest-environment jsdom
// The settings page's states that the window reaches only on a slow or lost connection (docs/design/SETTINGS.md,
// A setting's states): loading, and Ersa unable to reach Core.

import { cleanup, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { ArchiveView } from "../session/archiveInactive";
import { SettingsPage } from "./SettingsPage";

afterEach(cleanup);

const draw = (archive: ArchiveView, offline?: boolean) => render(createElement(SettingsPage, { archive, offline, onArchive: () => {} }));
const chip = () => screen.getByRole("button", { name: /^Archive inactive sessions:/ }) as HTMLButtonElement;

describe("Archive inactive sessions' chip", () => {
  it("says it is loading, with the turning ring, and does not open until the settings are read", () => {
    draw({ state: "loading" });
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: Loading…");
    expect(chip().querySelector(".context-ring")).not.toBeNull();
    expect(chip().disabled).toBe(true);
  });

  // A chip that becomes unavailable, or goes, with keyboard focus on it would drop focus to the window: the chosen
  // section name takes it, so the next Tab starts from the page.
  it("hands keyboard focus to the chosen section name when it becomes unavailable or goes", () => {
    const ready: ArchiveView = { state: "ready", value: 7 };
    const props = { onArchive: () => {} };
    const drawn = render(createElement(SettingsPage, { archive: ready, ...props }));
    const storage = screen.getByRole("button", { name: "Storage" });
    chip().focus();
    drawn.rerender(createElement(SettingsPage, { archive: ready, offline: true, ...props }));
    expect(document.activeElement).toBe(storage);

    drawn.rerender(createElement(SettingsPage, { archive: ready, ...props }));
    chip().focus();
    drawn.rerender(createElement(SettingsPage, { archive: { state: "error", message: "gone" }, ...props }));
    expect(document.activeElement).toBe(storage);
  });

  it("leaves focus the user put elsewhere when the chip becomes unavailable", () => {
    const ready: ArchiveView = { state: "ready", value: 7 };
    const drawn = render(createElement("div", null, createElement("button", null, "Elsewhere"), createElement(SettingsPage, { archive: ready, onArchive: () => {} })));
    chip().focus();
    screen.getByRole("button", { name: "Elsewhere" }).focus();
    drawn.rerender(createElement("div", null, createElement("button", null, "Elsewhere"), createElement(SettingsPage, { archive: ready, offline: true, onArchive: () => {} })));
    expect(document.activeElement?.textContent).toBe("Elsewhere");
  });

  it("keeps the value last read, unavailable, while Ersa can't reach Core", () => {
    draw({ state: "ready", value: 30 }, true);
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 30 days");
    expect(chip().disabled).toBe(true);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
