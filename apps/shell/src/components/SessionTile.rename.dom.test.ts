// @vitest-environment jsdom
// Renaming on a tile after Ersa loses Core mid-edit (FLOW-ORGANIZE-SESSIONS, step 2): Rename is unavailable then, so
// Enter asks Core nothing and the field says why.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { SessionTile } from "./SessionTile";

const session: Session = {
  id: "s1",
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  title: "Fix the build",
  state: "idle",
  created_at: "2026-11-01T09:00:00Z",
  archived: false,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
};

afterEach(cleanup);

describe("renaming on a tile after Ersa loses Core", () => {
  it("asks Core nothing on Enter, and says why Rename can't be used now", () => {
    const onSave = vi.fn(async () => true);
    const props = (unavailable?: string) => ({ session, active: false, onSelect: () => {}, rename: { onSave, onReset: () => {}, unavailable } });
    const { rerender } = render(createElement(SessionTile, props()));
    fireEvent.click(screen.getByRole("button", { name: "More for Fix the build" }));
    fireEvent.click(screen.getByRole("menuitem", { name: /^Rename/ }));
    const field = screen.getByRole("textbox", { name: "Session title" }) as HTMLInputElement;
    fireEvent.change(field, { target: { value: "Fix the flaky build" } });

    // The connection drops while the field is open.
    rerender(createElement(SessionTile, props("Available once Ersa reaches your sessions")));
    fireEvent.keyDown(field, { key: "Enter" });
    expect(onSave).not.toHaveBeenCalled();
    // The draft stays, with the reason beneath the tile.
    expect(field.value).toBe("Fix the flaky build");
    expect(document.querySelector(".session-rename-problem")?.textContent).toBe("Available once Ersa reaches your sessions");
    expect(document.querySelector(".session-rename-problem")?.getAttribute("role")).toBe("status");
  });
});
