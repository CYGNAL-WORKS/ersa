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

  it("keeps the value last read, unavailable, while Ersa can't reach Core", () => {
    draw({ state: "ready", value: 30 }, true);
    expect(chip().getAttribute("aria-label")).toBe("Archive inactive sessions: 30 days");
    expect(chip().disabled).toBe(true);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
