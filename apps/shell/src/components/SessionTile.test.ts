import { describe, expect, it } from "vitest";
import type { Session } from "../protocol/core-api.gen";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
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

  it("is one control: its chevron is a named mark, not a button", () => {
    const html = draw({ state: "waiting_user", pending: [{ kind: "permission", request_id: "r" }] });
    expect(html.match(/<button/g)).toHaveLength(1);
    expect(html).toContain('<span class="session-chevron" data-tone="caution" role="img" aria-label="Needs permission">');
    // The want on line two is named by the chevron already, so it isn't read twice.
    expect(html).toContain('<span class="session-phrase" data-tone="caution" aria-hidden="true">Needs permission</span>');
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
