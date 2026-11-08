// @vitest-environment jsdom
// Archive and Unarchive on a tile's menu, and an archived tile (FLOW-ARCHIVE-DELETE-SESSION; LAY-SHELL, A tile's menu
// and Archived; docs/design/RAIL.md, An archived tile).

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { SessionTile } from "./SessionTile";

const session = (archived: boolean): Session => ({
  id: "s1",
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  title: "Fix the build",
  state: "ended",
  created_at: "2026-11-01T09:00:00Z",
  archived,
  pinned: true,
  section_id: "sec",
  tile: { state: "ended", pending: [] },
});

function tile(archived: boolean, unavailable?: string) {
  const onChoose = vi.fn();
  render(
    createElement(SessionTile, {
      session: session(archived),
      active: false,
      unseen: false,
      onSelect: () => {},
      rename: { onSave: async () => true, onReset: () => {} },
      archive: { onChoose, unavailable },
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "More for Fix the build" }));
  return onChoose;
}

afterEach(cleanup);

describe("Archive and Unarchive on a tile's menu", () => {
  it("offers Archive after Rename on a session that is not archived", () => {
    const onChoose = tile(false);
    const items = screen.getAllByRole("menuitem").map((i) => i.textContent);
    expect(items).toEqual(["RenameF2", "Archive"]);
    fireEvent.click(screen.getByRole("menuitem", { name: "Archive" }));
    expect(onChoose).toHaveBeenCalledTimes(1);
  });

  // verifies: FLOW-ARCHIVE-DELETE-SESSION-R03
  it("offers Unarchive on an archived session, whose tile carries the archive mark named Archived", () => {
    const onChoose = tile(true);
    expect(screen.getByRole("img", { name: "Archived" })).toBeTruthy();
    fireEvent.click(screen.getByRole("menuitem", { name: "Unarchive" }));
    expect(onChoose).toHaveBeenCalledTimes(1);
  });

  it("shows Archive unavailable with its reason, and choosing it does nothing", () => {
    const onChoose = tile(false, "Available once Ersa reaches your sessions");
    const item = screen.getByRole("menuitem", { name: /^Archive/ });
    expect(item.getAttribute("aria-disabled")).toBe("true");
    expect(item.textContent).toContain("Available once Ersa reaches your sessions");
    fireEvent.click(item);
    expect(onChoose).not.toHaveBeenCalled();
  });
});
