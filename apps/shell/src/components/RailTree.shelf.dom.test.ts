// @vitest-environment jsdom
// The rail with a shelf, in a DOM, driven by keyboard and pointer: where focus lands as the shelf opens, closes, and
// is answered (FLOW-APPROVE-PERMISSION, Opening and closing).

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement, useEffect, useState } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { PermissionRequested } from "../protocol/agent-events.gen";
import type { Session } from "../protocol/core-api.gen";
import { shelfAfter } from "../session/shelf";
import { RailTree } from "./RailTree";
import { Shelf } from "./Shelf";

const request: PermissionRequested = {
  type: "permission.requested",
  request_id: "r1",
  tool_use_id: "t1",
  tool_name: "Write",
  input: { file_path: "notes.txt" },
  suggestions: [],
  flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
};

const session = (id: string, title: string, created_at: string, pending: Session["tile"]["pending"]): Session => ({
  id,
  title,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
  state: pending.length > 0 ? "waiting_user" : "idle",
  created_at,
  tile: { state: pending.length > 0 ? "waiting_user" : "idle", pending },
});

const waiting = session("a", "Write the notes", "2026-11-06T09:00:00Z", [{ kind: "permission", request_id: "r1" }]);
const active = session("b", "Fix the build", "2026-11-06T10:00:00Z", []);

/** What the rail's owner holds: the sessions, and which shelf is open, changed as the window changes it. */
let list: (sessions: Session[]) => void = () => {};
const decided: string[] = [];

function Rail() {
  const [sessions, setSessions] = useState([waiting, active]);
  const [open, setOpen] = useState<string>();
  list = setSessions;
  useEffect(() => setOpen((o) => shelfAfter(o, { type: "sessions", sessions, activeId: "b" })), [sessions]);
  return createElement(
    "div",
    null,
    createElement(RailTree, {
      projects: { state: "ready", projects: [{ id: "p", host_id: "local", path: "D:/code/p", name: "p", vcs: "git", trusted: true }] },
      sessions: { state: "ready", sessions },
      workspaces: { w: { project_id: "p" } },
      activeId: "b",
      onSelect: () => {},
      shelf: open
        ? {
            id: open,
            tone: "caution",
            render: (domId: string) =>
              createElement(Shelf, {
                id: domId,
                tile: { title: "Write the notes" },
                shows: { state: "permission", request },
                live: true,
                onDecide: (_: string, decision) => decided.push(String(decision)),
              }),
          }
        : undefined,
      onChevron: (id: string) => setOpen((o) => shelfAfter(o, { type: "chevron", id, activeId: "b" })),
      onCloseShelf: (id: string) => setOpen((o) => shelfAfter(o, { type: "close", id })),
    }),
    createElement("button", { type: "button" }, "Elsewhere"),
  );
}

const chevron = () => screen.getByRole("button", { name: "Request from Write the notes" });
const shelf = () => screen.queryByRole("dialog", { name: "Request from Write the notes" });

function openShelf() {
  render(createElement(Rail));
  chevron().focus();
  fireEvent.click(chevron());
  return shelf() as HTMLElement;
}

beforeEach(() => {
  // With reduced motion on, a closing shelf goes at once, so a closed one is gone from the page.
  window.matchMedia = ((query: string) => ({ matches: query.includes("reduce"), media: query })) as unknown as typeof window.matchMedia;
  // The open tile measures where its shelf joins it; jsdom lays nothing out.
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
  decided.length = 0;
});

afterEach(cleanup);

describe("the shelf and keyboard focus", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R05
  it("moves focus to the shelf itself as it opens, not to an answer", () => {
    const open = openShelf();
    expect(document.activeElement).toBe(open);
    expect(open.getAttribute("tabindex")).toBe("-1");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R06
  it("closes on Escape in the shelf or on its chevron, or on its chevron again, with focus on the chevron", () => {
    const open = openShelf();
    fireEvent.keyDown(open, { key: "Escape" });
    expect(shelf()).toBeNull();
    expect(document.activeElement).toBe(chevron());
    expect(chevron().getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(chevron());
    fireEvent.keyDown(chevron(), { key: "Escape" });
    expect(shelf()).toBeNull();
    expect(document.activeElement).toBe(chevron());

    fireEvent.click(chevron());
    expect(shelf()).not.toBeNull();
    fireEvent.click(chevron());
    expect(shelf()).toBeNull();
    expect(document.activeElement).toBe(chevron());
  });

  // verifies: FLOW-APPROVE-PERMISSION-R07
  it("closes when the user chooses outside the shelf and its tile, leaving focus where they put it", () => {
    openShelf();
    // Inside the shelf, or on its tile, it stays.
    fireEvent.pointerDown(shelf() as HTMLElement);
    fireEvent.pointerDown(screen.getByRole("button", { name: /^(?!Request from).*Write the notes/ }));
    expect(shelf()).not.toBeNull();

    // A press that takes no focus closes it too, and gives focus to nothing: not back to the chevron.
    fireEvent.pointerDown(document.body);
    expect(shelf()).toBeNull();
    expect(document.activeElement).toBe(document.body);

    // A press on something that takes focus: the press itself places focus, so closing places none.
    fireEvent.click(chevron());
    fireEvent.pointerDown(screen.getByRole("button", { name: "Elsewhere" }));
    expect(shelf()).toBeNull();
    expect(document.activeElement).not.toBe(chevron());
  });

  // verifies: FLOW-APPROVE-PERMISSION-R07
  it("closes when keyboard focus leaves the shelf and its tile, and leaves focus where it went", () => {
    const open = openShelf();
    // To the tile, focus stays with them, and the shelf with it.
    const tile = screen.getByRole("button", { name: /^(?!Request from).*Write the notes/ });
    act(() => tile.focus());
    expect(shelf()).not.toBeNull();
    act(() => open.focus());
    const elsewhere = screen.getByRole("button", { name: "Elsewhere" });
    act(() => elsewhere.focus());
    expect(shelf()).toBeNull();
    expect(document.activeElement).toBe(elsewhere);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R08
  it("closes once nothing is pending, with focus from the shelf on the tile, since its chevron is gone", () => {
    openShelf();
    act(() => list([{ ...waiting, state: "running", tile: { state: "running", pending: [] } }, active]));
    expect(shelf()).toBeNull();
    expect(screen.queryByRole("button", { name: "Request from Write the notes" })).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /^(?!Request from).*Write the notes/ }));
  });

  // verifies: FLOW-APPROVE-PERMISSION-R08
  it("leaves focus where it is when the shelf closes while focus is elsewhere", () => {
    openShelf();
    const tile = screen.getByRole("button", { name: /^(?!Request from).*Write the notes/ });
    act(() => tile.focus());
    act(() => list([{ ...waiting, state: "running", tile: { state: "running", pending: [] } }, active]));
    expect(shelf()).toBeNull();
    expect(document.activeElement).toBe(tile);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R06
  it("puts focus on the chevron while the shelf slides back, out of reach", () => {
    window.matchMedia = ((query: string) => ({ matches: false, media: query })) as unknown as typeof window.matchMedia;
    const open = openShelf();
    fireEvent.keyDown(open, { key: "Escape" });
    // Still on the page as it slides back, but no longer the open shelf.
    expect(open.isConnected).toBe(true);
    expect(open.parentElement?.hasAttribute("inert")).toBe(true);
    expect(document.activeElement).toBe(chevron());
  });

  it("keeps focus in the shelf when an answer makes the answers unavailable", () => {
    const open = openShelf();
    const allow = screen.getByRole("button", { name: "Allow" });
    act(() => allow.focus());
    fireEvent.click(allow);
    expect(decided).toEqual(["allow"]);
    expect(document.activeElement).toBe(open);
    expect(shelf()).not.toBeNull();
  });
});
