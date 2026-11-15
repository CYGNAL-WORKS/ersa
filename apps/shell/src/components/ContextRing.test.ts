import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { UsageWindow } from "../session/limits";
import { ContextRing } from "./ContextRing";

// LAY-SHELL, Context; docs/design/COMPOSER.md, The context ring: quiet up to half, caution past half, danger past
// four fifths, on a tile and in the session controls. A layout brief, so no IDs.
const arc = (html: string) => html.match(/class="context-ring-used"[^>]*stroke-dasharray="([\d.]+) 100"/)?.[1];
const readout = (html: string) => html.match(/context-ring-readout">([^<]*)</)?.[1];
const band = (html: string) => html.match(/data-band="(\w+)"/)?.[1] ?? "quiet";

describe("the context ring", () => {
  const draw = (used: number | undefined, size?: "ring" | "ring-sm") => renderToStaticMarkup(createElement(ContextRing, { used, size }));
  const unreported = "Context isn&#x27;t reported yet";

  it("with no reading is the track alone, with no figure, and says context isn't reported yet", () => {
    const html = draw(undefined);
    expect(arc(html)).toBe("0");
    expect(readout(html)).toBeUndefined();
    expect(band(html)).toBe("quiet");
    expect(html).toContain(`aria-label="${unreported}" title="${unreported}"`);
  });

  it("before a session, with no reading, says none is used yet", () => {
    const html = renderToStaticMarkup(createElement(ContextRing, { beforeSession: true }));
    expect(html).toContain('aria-label="Context: none used yet"');
  });

  it("with a reading of nothing used is the track alone and reads 0%", () => {
    const html = draw(0);
    expect(arc(html)).toBe("0");
    expect(readout(html)).toBe("0%");
    expect(html).toContain('aria-label="Context: 0 percent used"');
  });

  it("draws a small share as a twentieth of the ring, reading <1%", () => {
    const html = draw(0.003);
    expect(arc(html)).toBe("5");
    expect(readout(html)).toBe("&lt;1%");
    expect(band(html)).toBe("quiet");
    expect(html).toContain('aria-label="Context: less than 1 percent used" title="Context: less than 1 percent used"');
    // Just under 1 percent rounds to 1 but is still under it.
    expect(readout(draw(0.006))).toBe("&lt;1%");
    expect(readout(draw(0.01))).toBe("1%");
  });

  it("is quiet up to half, caution past half and danger past four fifths, by the figure it reads", () => {
    for (const [used, figure, expected] of [
      [0.5, "50%", "quiet"],
      [0.504, "50%", "quiet"],
      [0.51, "51%", "caution"],
      [0.8, "80%", "caution"],
      [0.81, "81%", "danger"],
    ] as const) {
      const html = draw(used);
      expect([readout(html), band(html)]).toEqual([figure, expected]);
      expect(Number(arc(html))).toBe(Math.round(used * 100));
      expect(band(draw(used, "ring-sm"))).toBe(expected);
    }
  });

  it("never shows more than full", () => {
    const html = draw(1.4);
    expect(arc(html)).toBe("100");
    expect(readout(html)).toBe("100%");
    expect(band(html)).toBe("danger");
    expect(html).toContain('aria-label="Context: 100 percent used"');
  });

  // verifies: FLOW-SEND-MESSAGE-R16
  it("as a send's copy gauge, fills to the share itself, never turns a band and has no readout or name of its own", () => {
    const html = renderToStaticMarkup(createElement(ContextRing, { used: 0.95, mark: true }));
    expect(html).not.toContain("data-band");
    expect(html).not.toContain("context-ring-readout");
    expect(html).toContain('aria-hidden="true"');
    expect(html).not.toContain("aria-label");
    expect(html).not.toContain("title=");
    expect(arc(renderToStaticMarkup(createElement(ContextRing, { used: 0.01, mark: true })))).toBe("1");
  });

  it("draws a tile's ring smaller, with no readout", () => {
    const html = draw(0.42, "ring-sm");
    expect(html).toContain('viewBox="0 0 12 12"');
    expect(html).not.toContain("context-ring-readout");
    expect(html).toContain('aria-label="Context: 42 percent used"');
  });
});

// LAY-SHELL, Context; docs/design/COMPOSER.md, The context ring (issue #422): in the session controls the ring reads
// the nearest limit and lists every reading with when each window resets.
describe("the session controls' ring", () => {
  afterEach(() => vi.useRealTimers());
  const limits = (used: number, windows: UsageWindow[]) => renderToStaticMarkup(createElement(ContextRing, { used, windows }));
  const lines = (html: string) =>
    [...html.matchAll(/<span class="context-ring-tip-line"><span>([^<]*)<\/span>(?:<span class="context-ring-tip-resets">([^<]*)<\/span>)?/g)].map(
      ([, label, resets]) => (resets ? `${label} | ${resets.replace(/ /g, " ")}` : label),
    );

  it("reads a window in caution or danger by the whole percent it shows, and names it", () => {
    const caution = limits(0.3, [{ id: "seven_day", label: "Weekly · all models", used: 0.62 }]);
    expect(band(caution)).toBe("caution");
    expect(readout(caution)).toBe("62%");
    expect(caution).toContain('aria-label="Weekly · all models: 62 percent used"');
    // 80.9 percent reads 80, rounded down, so it is still caution: the band follows the figure.
    expect(band(limits(0.3, [{ id: "five_hour", label: "5-hour limit", used: 0.809 }]))).toBe("caution");
    const danger = limits(0.3, [{ id: "five_hour", label: "5-hour limit", used: 0.84 }]);
    expect(band(danger)).toBe("danger");
    expect(arc(danger)).toBe("84");
  });

  it("lists the context, then each window as reported, with when it resets in each of its forms", () => {
    vi.useFakeTimers();
    // Sunday 7:00 AM in New York, where the tests run.
    vi.setSystemTime(new Date("2026-11-15T12:00:00Z"));
    const html = limits(0.3, [
      { id: "seven_day", label: "Weekly · all models", used: 0.62, resets_at: "2026-11-18T19:00:00Z" },
      { id: "five_hour", label: "5-hour limit", used: 0.84, resets_at: "2026-11-15T13:15:00Z" },
      { id: "overage", label: "Weekly · overage", used: 0.1, resets_at: "2026-11-15T12:15:00Z" },
      { id: "opus", label: "opus_week", used: 0.05 },
    ]);
    expect(lines(html)).toEqual([
      "Context: 30 percent used",
      "Weekly · all models: 62 percent used | Resets Wed 2:00 PM",
      "5-hour limit: 84 percent used | Resets in 1 hr 15 min",
      "Weekly · overage: 10 percent used | Resets in 15 min",
      "opus_week: 5 percent used",
    ]);
    expect(html).toContain('role="tooltip"');
    expect(html).toContain('tabindex="0"');
  });

  it("with no windows reads and lists the context alone", () => {
    const html = limits(0.42, []);
    expect(html).toContain('aria-label="Context: 42 percent used"');
    expect(lines(html)).toEqual(["Context: 42 percent used"]);
    expect(html).not.toContain("title=");
  });
});
