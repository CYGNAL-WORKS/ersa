// @vitest-environment jsdom
// Keyboard focus as the rail redraws (docs/design/RAIL.md: a tile that leaves the rail, and Menus): a tile that
// leaves passes focus on, and one drawn afresh in its new group takes it back, only while the user has left focus in
// the rail. A design rule, so no requirement IDs.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Project, Session } from "../protocol/core-api.gen";
import type { Moving } from "../session/sections";
import { RailTree } from "./RailTree";

const session = (id: string, title: string, created_at: string, pinned = false): Session => ({
  id,
  title,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at,
  archived: false,
  pinned,
  unseen: false,
  tile: { state: "idle", pending: [] },
});

const project: Project = { id: "p", host_id: "local", path: "D:/p", name: "p", vcs: "git", trusted: true };

// Newest first: Tidy, Write, Fix.
const three = [session("s1", "Fix the build", "2026-11-01T09:00:00Z"), session("s2", "Write the notes", "2026-11-02T09:00:00Z"), session("s3", "Tidy the docs", "2026-11-03T09:00:00Z")];

const rail = (sessions: Session[], more: Partial<Parameters<typeof RailTree>[0]> = {}) =>
  createElement(RailTree, {
    projects: { state: "ready", projects: [project] },
    workspaces: { w: { project_id: "p" } },
    sessions: { state: "ready", sessions },
    onSelect: () => {},
    ...more,
  });

const tile = (title: string) => [...document.querySelectorAll<HTMLElement>(".session")].find((el) => el.textContent?.includes(title));

/** A press on the page where nothing takes focus: the browser moves focus to nothing, the tile still drawn. */
const pressPage = async () => {
  fireEvent.pointerDown(document.body);
  (document.activeElement as HTMLElement | null)?.blur();
  await act(() => Promise.resolve());
};

afterEach(cleanup);

describe("a tile that leaves the rail", () => {
  it("passes focus to the next tile while focus is in it", () => {
    const { rerender } = render(rail(three));
    tile("Write the notes")?.focus();
    rerender(rail(three.filter((s) => s.id !== "s2")));
    expect(document.activeElement).toBe(tile("Fix the build"));
  });

  it("takes nothing back once the user pressed somewhere that takes no focus", async () => {
    const { rerender } = render(rail(three));
    tile("Write the notes")?.focus();
    await pressPage();
    expect(document.activeElement).toBe(document.body);
    rerender(rail(three.filter((s) => s.id !== "s2")));
    expect(document.activeElement).toBe(document.body);
  });
});

describe("a tile pinned from its menu", () => {
  const pinning = (moving: Record<string, Moving> | undefined, sessions = three) => rail(sessions, { onPin: () => {}, onRename: () => Promise.resolve(true), moving });

  it("takes focus back in its new place once Core has the pin", async () => {
    const { rerender } = render(pinning(undefined));
    tile("Write the notes")?.focus();
    fireEvent.keyDown(tile("Write the notes") as HTMLElement, { key: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    rerender(pinning({ s2: { state: "sending", change: "pin" } }));
    // Drawn afresh in the Pinned group, so the focus its menu gave back is lost.
    rerender(pinning(undefined, three.map((s) => (s.id === "s2" ? { ...s, pinned: true } : s))));
    expect(document.activeElement).toBe(tile("Write the notes"));
  });

  it("takes nothing back once the user pressed somewhere that takes no focus while it saved", async () => {
    const { rerender } = render(pinning(undefined));
    tile("Write the notes")?.focus();
    fireEvent.keyDown(tile("Write the notes") as HTMLElement, { key: "F10", shiftKey: true });
    fireEvent.click(screen.getByRole("menuitem", { name: "Pin" }));
    rerender(pinning({ s2: { state: "sending", change: "pin" } }));
    await pressPage();
    rerender(pinning(undefined, three.map((s) => (s.id === "s2" ? { ...s, pinned: true } : s))));
    expect(document.activeElement).toBe(document.body);
  });
});
