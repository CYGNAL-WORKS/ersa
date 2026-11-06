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

  // Workspace headings stand until the rail's tree is built (LAY-SHELL, The rail), so this cites no requirement.
  it("shows a session's tile under its workspace's name", () => {
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
    expect(html.indexOf('aria-label="ersa, ersa/fix-build"')).toBeLessThan(html.indexOf('aria-label="ersa, Root workspace"'));
  });

  it("keeps a workspace's heading line while its name is read, so the tiles do not move when it arrives", () => {
    const render = (workspaces: Record<string, { project: string; name: string; path?: boolean }>) =>
      renderToStaticMarkup(createElement(SessionList, { view: { state: "ready", sessions }, onSelect: () => {}, workspaces }));
    const unnamed = render({});
    // One heading per group either way: the same lines, before and after the names.
    const headings = (html: string) => html.match(/<h3 class="sessions-workspace-name"/g)?.length;
    expect(headings(unnamed)).toBe(2);
    expect(unnamed).toContain('<h3 class="sessions-workspace-name" aria-hidden="true"> </h3>');
    expect(headings(render({ "w-root": { project: "ersa", name: "Root workspace" } }))).toBe(2);
  });

  it("sets a workspace named by its path as a path", () => {
    const html = renderToStaticMarkup(
      createElement(SessionList, {
        view: { state: "ready", sessions },
        onSelect: () => {},
        workspaces: { "w-branch": { project: "ersa", name: "D:\ws\ersa-1", path: true } },
      }),
    );
    expect(html).toContain('<span class="mono">D:\ws\ersa-1</span>');
  });
});

// LAY-SHELL, The rail (Tiles, Needs you); docs/design/RAIL.md, A tile. A layout brief, so no requirement IDs.
describe("a session's tile", () => {
  const draw = (tile: Partial<Session["tile"]>, activeId?: string) =>
    renderToStaticMarkup(
      createElement(SessionList, {
        view: { state: "ready", sessions: [{ ...at("s", "2026-11-01T09:00:00.000Z"), title: "Fix the build", tile: { state: "idle", pending: [], ...tile } }] },
        activeId,
        onSelect: () => {},
      }),
    );

  // verifies: FLOW-APPROVE-PERMISSION-R22
  it("makes the chevron of a session that isn't the active one a button named for its shelf, saying whether it is open", () => {
    const html = draw({ state: "waiting_user", pending: [{ kind: "permission", request_id: "r" }] });
    expect(html).toContain(
      '<button type="button" class="session-chevron-button" data-tone="caution" aria-label="Request from Fix the build" aria-haspopup="dialog" aria-expanded="false">',
    );
    // The chevron's name no longer says the want, so line two's is read with the tile.
    expect(html).toContain('<span class="session-phrase" data-tone="caution">Needs permission</span>');
    const open = renderToStaticMarkup(
      createElement(SessionList, {
        view: { state: "ready", sessions: [{ ...at("s", "2026-11-01T09:00:00.000Z"), title: "Fix the build", tile: { state: "waiting_user", pending: [{ kind: "permission", request_id: "r" }] } }] },
        onSelect: () => {},
        shelf: { id: "s", tone: "caution", render: (domId: string) => createElement("div", { id: domId, className: "shelf" }) },
      }),
    );
    expect(open).toMatch(/aria-expanded="true" aria-controls="([^"]+)">.*<div id="\1" class="shelf">/);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R26
  it("leaves the active session's chevron a named mark that opens nothing", () => {
    const html = draw({ state: "waiting_user", pending: [{ kind: "permission", request_id: "r" }] }, "s");
    expect(html.match(/<button/g)).toHaveLength(1);
    expect(html).toContain('<span class="session-chevron" data-tone="caution" role="img" aria-label="Needs permission">');
    // The want on line two is named by the chevron already, so it isn't read twice.
    expect(html).toContain('<span class="session-phrase" data-tone="caution" aria-hidden="true">Needs permission</span>');
  });

  // verifies: FLOW-APPROVE-PERMISSION-R03
  it("draws one shelf, beside its own tile, which takes the shelf's tone", () => {
    const waiting = (id: string, created: string) => ({
      ...at(id, created),
      title: id,
      tile: { state: "waiting_user" as const, pending: [{ kind: "question" as const, request_id: `r-${id}` }] },
    });
    const html = renderToStaticMarkup(
      createElement(SessionList, {
        view: { state: "ready", sessions: [waiting("a", "2026-11-01T09:00:00.000Z"), waiting("b", "2026-11-01T10:00:00.000Z")] },
        onSelect: () => {},
        shelf: { id: "a", tone: "caution", render: () => createElement("div", { className: "shelf" }) },
      }),
    );
    expect(html.match(/class="shelf"/g)).toHaveLength(1);
    expect(html.match(/<li class="session-item" data-shelf="true" data-tone="caution">.*?<\/li>/)?.[0]).toContain('aria-label="Request from a"');
    // A question's tile is the accent, until its shelf shows something in the caution tone.
    expect(html).toContain('data-tone="caution" aria-label="Request from a"');
    expect(html).toContain('data-tone="accent" aria-label="Request from b"');
  });

  it("names the backend's icon", () => {
    expect(draw({})).toContain('<span class="session-backend" role="img" aria-label="claude-code">');
  });

  it("puts the cost after the phrase, with a dot only between the two", () => {
    expect(draw({ state: "error", cost_usd_total: 0.02 })).toContain('<span class="session-detail"><span class="session-phrase">Error</span> · $0.02</span>');
    expect(draw({ state: "running", cost_usd_total: 0.19 })).toContain('<span class="session-detail">$0.19</span>');
    expect(draw({ state: "error" })).toContain('<span class="session-detail"><span class="session-phrase">Error</span></span>');
  });

  it("keeps line two when it has nothing to show, and shows no ring until one is reported", () => {
    const html = draw({ state: "idle" });
    expect(html).toContain('<span class="session-detail"></span>');
    expect(html).not.toContain("context-ring");
    expect(draw({ context_tokens: 10, context_window: 100 })).toContain('aria-label="Context: 10 percent used"');
  });

  it("turns the ring caution from 85 percent used", () => {
    expect(draw({ context_tokens: 85, context_window: 100 })).toContain('data-size="ring-sm" data-near-full="true"');
    expect(draw({ context_tokens: 84, context_window: 100 })).not.toContain("data-near-full");
  });

  it("draws each mark in the tone the mapping gives", () => {
    expect(draw({ state: "running" })).toContain('<span class="glyph session-glyph" data-tone="accent" role="img" aria-label="Working">');
    expect(draw({ state: "disconnected" })).toContain('<span class="glyph session-glyph" data-tone="faint" aria-hidden="true">');
    expect(draw({ state: "error" })).toContain('<span class="glyph session-glyph" data-tone="danger" aria-hidden="true">');
  });

  it("marks the active tile as the current one", () => {
    expect(draw({}, "s")).toContain('aria-current="true"');
    expect(draw({})).not.toContain("aria-current");
  });
});
