// The lookup at its public call (docs/design/CONTENT.md, "The string catalog":
// Values and Plurals). The typecheck half lives in lookup.typecheck.ts.

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { lookupIn } from "./lookup";
import { sample } from "./lookup.typecheck";

const t = lookupIn(sample, "en-US");

describe("the lookup (The string catalog: Values)", () => {
  it("returns a message without values as it's written", () => {
    expect(t("quit.title")).toBe("Quit Ersa?");
  });

  it("fills a value, in a nested sub-area", () => {
    expect(t("rail.core.unreachableError", { host: "devbox" })).toBe("Core isn't connected on devbox. Ersa is trying again.");
  });

  it("writes a number as a numeral, grouped for the locale", () => {
    expect(t("quit.detail", { count: 1200 })).toMatch(/^1,200 sessions are/);
  });

  it("keeps an element value in its place in the sentence", () => {
    const readout = createElement("span", { className: "readout" }, "devbox");
    const node = t("rail.core.unreachableError", { host: readout });
    expect(renderToStaticMarkup(createElement("p", null, node))).toBe(
      '<p>Core isn&#x27;t connected on <span class="readout">devbox</span>. Ersa is trying again.</p>',
    );
  });

  it("refuses a key that leads to no message", () => {
    // Cast past the types, as a key built at runtime would be.
    expect(() => (t as unknown as (key: string) => string)("rail.core")).toThrow(/no message at "rail.core"/);
  });

  it("refuses a missing value rather than showing a gap", () => {
    const loose = t as unknown as (key: string, values?: object) => string;
    expect(() => loose("rail.core.unreachableError", {})).toThrow(/needs a value for \{host\}/);
  });

  it("refuses a plural without a count", () => {
    const loose = t as unknown as (key: string, values?: object) => string;
    expect(() => loose("quit.detail", {})).toThrow(/has plural forms and needs a count/);
  });
});

describe("plural selection (The string catalog: Plurals)", () => {
  it("chooses `one` for a count of 1, whole message and all", () => {
    expect(t("quit.detail", { count: 1 })).toBe(
      "1 session is working and will stop. Closing the window instead keeps it running.",
    );
  });

  it("chooses `other` for any other count, 0 included", () => {
    expect(t("quit.detail", { count: 2 })).toBe(
      "2 sessions are working and will stop. Closing the window instead keeps them running.",
    );
    expect(t("quit.detail", { count: 0 })).toMatch(/^0 sessions are working/);
  });

  it("fills a plural form's other values beside the count", () => {
    expect(t("quit.stoppingOn", { count: 1, host: "devbox" })).toBe("Stopping 1 session on devbox…");
    expect(t("quit.stoppingOn", { count: 3, host: "devbox" })).toBe("Stopping 3 sessions on devbox…");
  });

  it("falls back to `other` for a category the message doesn't write", () => {
    // Welsh has a `two` category; the sample writes only `one` and `other`.
    const cy = lookupIn(sample, "cy");
    expect(cy("quit.stoppingOn", { count: 2, host: "devbox" })).toBe("Stopping 2 sessions on devbox…");
  });
});

describe("a component reading the catalog", () => {
  // A stand-in for a component such as the quit question: `{count}` chooses
  // the form, and `{readout}` sets the number as a styled readout in its place.
  function Busy({ busy }: { busy: number }) {
    const readout = createElement("span", { className: "readout" }, busy);
    return createElement("p", { "aria-label": t("quit.title") }, t("quit.busySummary", { count: busy, readout }));
  }

  it("renders a plural form with an element value, and a text value in a prop", () => {
    expect(renderToStaticMarkup(createElement(Busy, { busy: 1 }))).toBe(
      '<p aria-label="Quit Ersa?"><span class="readout">1</span> session is working.</p>',
    );
    expect(renderToStaticMarkup(createElement(Busy, { busy: 4 }))).toBe(
      '<p aria-label="Quit Ersa?"><span class="readout">4</span> sessions are working.</p>',
    );
  });
});
