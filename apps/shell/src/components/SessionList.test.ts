import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { SessionList, byWorkspace, newestFirst } from "./SessionList";

const at = (id: string, created_at: string): Session => ({
  id,
  workspace_id: "w",
  host_id: "local",
  backend: "claude-code",
  model: "opus",
  effort: "medium",
  permission_mode: "default",
  state: "idle",
  created_at,
  tile: { state: "idle", pending: [] },
});

describe("newestFirst", () => {
  it("orders by creation time, newest first, without changing its input", () => {
    const sessions = [at("a", "2026-11-01T09:00:00.000Z"), at("b", "2026-11-01T10:00:00.000Z"), at("c", "2026-11-01T09:30:00.000Z")];
    expect(newestFirst(sessions).map((s) => s.id)).toEqual(["b", "c", "a"]);
    expect(sessions.map((s) => s.id)).toEqual(["a", "b", "c"]);
  });
});

describe("the rail's sessions by workspace", () => {
  const ws = (s: Session, workspace_id: string): Session => ({ ...s, workspace_id });
  const sessions = [
    ws(at("old", "2026-11-01T09:00:00.000Z"), "w-root"),
    ws(at("new", "2026-11-01T11:00:00.000Z"), "w-branch"),
    ws(at("mid", "2026-11-01T10:00:00.000Z"), "w-root"),
  ];

  it("groups by workspace, the workspace with the newest session first", () => {
    expect(byWorkspace(sessions).map(([id, list]) => [id, list.map((s) => s.id)])).toEqual([
      ["w-branch", ["new"]],
      ["w-root", ["mid", "old"]],
    ]);
  });

  // verifies: FLOW-NEW-SESSION-R09
  it("shows a started session's tile, active, under its workspace's name", () => {
    const html = renderToStaticMarkup(
      createElement(SessionList, {
        view: { state: "ready", sessions },
        activeId: "new",
        onSelect: () => {},
        workspaces: { "w-root": { project: "ersa", name: "Root workspace" }, "w-branch": { project: "ersa", name: "ersa/fix-build" } },
      }),
    );
    const branch = html.slice(html.indexOf('aria-label="ersa, ersa/fix-build"'), html.indexOf('aria-label="ersa, Root workspace"'));
    expect(branch).toContain("ersa/fix-build</span></h3>");
    expect(branch).toMatch(/aria-current="true"/);
    expect(html.indexOf('aria-label="ersa, ersa/fix-build"')).toBeLessThan(html.indexOf('aria-label="ersa, Root workspace"'));
  });
});
