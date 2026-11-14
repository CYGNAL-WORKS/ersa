import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { RailTree } from "./RailTree";
import { SessionTile } from "./SessionTile";

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
  archived: false,
  pinned: false,
  unseen: false,
  tile: { state: "idle", pending: [] },
});

// LAY-SHELL, The rail (Tiles, Needs you); docs/design/RAIL.md, A tile. A layout brief, so no requirement IDs.
describe("a session's tile", () => {
  const draw = (tile: Partial<Session["tile"]>, activeId?: string) =>
    renderToStaticMarkup(
      createElement(SessionTile, {
        session: { ...at("s", "2026-11-01T09:00:00.000Z"), title: "Fix the build", tile: { state: "idle", pending: [], ...tile } },
        active: activeId === "s",
        unseen: false,
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
      createElement(RailTree, {
        projects: { state: "ready", projects: [] },
        workspaces: {},
        sessions: { state: "ready", sessions: [{ ...at("s", "2026-11-01T09:00:00.000Z"), title: "Fix the build", tile: { state: "waiting_user", pending: [{ kind: "permission", request_id: "r" }] } }] },
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
      createElement(RailTree, {
        projects: { state: "ready", projects: [] },
        workspaces: {},
        sessions: { state: "ready", sessions: [waiting("a", "2026-11-01T09:00:00.000Z"), waiting("b", "2026-11-01T10:00:00.000Z")] },
        onSelect: () => {},
        shelf: { id: "a", tone: "caution", render: () => createElement("div", { className: "shelf" }) },
      }),
    );
    expect(html.match(/class="shelf"/g)).toHaveLength(1);
    // The open tile names its own anchor, and the shelf's layer is placed by it.
    const open = html.match(/<li class="session-item" data-slide="session:a" data-shelf="true" data-tone="caution" style="anchor-name:(--[^"]+)"[^>]*>.*?<\/li>/);
    expect(open?.[0]).toContain('aria-label="Request from a"');
    expect(open?.[0]).toContain(`<div class="shelf-layer" style="position-anchor:${open?.[1]}">`);
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
    expect(draw({ state: "running" })).toContain('<span class="glyph session-glyph" data-tone="accent" data-breath="loop" role="img" aria-label="Working">');
    expect(draw({ state: "disconnected" })).toContain('<span class="glyph session-glyph" data-tone="faint" aria-hidden="true">');
    expect(draw({ state: "error" })).toContain('<span class="glyph session-glyph" data-tone="danger" aria-hidden="true">');
  });

  it("marks the active tile as the current one", () => {
    expect(draw({}, "s")).toContain('aria-current="true"');
    expect(draw({})).not.toContain("aria-current");
  });
});
