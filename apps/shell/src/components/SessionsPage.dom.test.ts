// @vitest-environment jsdom
// Where keyboard focus goes when every row leaves at once.

import { cleanup, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { SessionsPage } from "./SessionsPage";

afterEach(cleanup);

const session: Session = {
  id: "s1",
  title: "Fix the build",
  workspace_id: "w1",
  host_id: "local",
  backend: "claude-code",
  permission_mode: "default",
  state: "idle",
  created_at: "2026-11-01T09:00:00Z",
  archived: false,
  pinned: false,
  tile: { state: "idle", pending: [] },
};

const page = (more: Partial<Parameters<typeof SessionsPage>[0]>) =>
  createElement(SessionsPage, { sessions: { state: "ready", sessions: [session] }, workspaces: {}, onOpen: () => {}, onNewSession: () => {}, ...more });

const focusRow = () => document.querySelector<HTMLElement>(".sessions-page-row .session")?.focus();

describe("focus when every row leaves", () => {
  it("goes to New session while it is available", () => {
    const { rerender } = render(page({}));
    focusRow();
    rerender(page({ sessions: { state: "ready", sessions: [] } }));
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "New session" }));
  });

  it("goes to the chosen tab when Ersa loses Core and New session is unavailable", () => {
    const { rerender } = render(page({}));
    focusRow();
    rerender(page({ sessions: { state: "unavailable" }, offline: true }));
    expect(document.activeElement).toBe(screen.getByRole("tab", { name: "Active" }));
  });
});
