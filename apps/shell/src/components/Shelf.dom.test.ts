// @vitest-environment jsdom
// The shelf in a DOM as it moves from one request to the next: what assistive technology hears, and the contents'
// cross-fade (docs/design/RAIL.md, The shelf: Motion).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { PermissionRequested } from "../protocol/agent-events.gen";
import type { ShelfShows } from "../session/shelf";
import { Shelf } from "./Shelf";

const flags = { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false };
const request = (request_id: string, description: string): PermissionRequested => ({
  type: "permission.requested",
  request_id,
  tool_use_id: `t-${request_id}`,
  tool_name: "Bash",
  description,
  input: { command: "pnpm test" },
  suggestions: [],
  flags,
});

const shelf = (shows: ShelfShows) => createElement(Shelf, { tile: { title: "Fix the build" }, shows, live: true, onDecide: () => {} });
const contents = () => document.querySelector(".shelf-contents") as HTMLElement;
const reduce = (on: boolean) => {
  window.matchMedia = ((query: string) => ({ matches: on && query.includes("reduce"), media: query })) as unknown as typeof window.matchMedia;
};

beforeEach(() => reduce(false));
afterEach(cleanup);

describe("the shelf moving to the next request", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R24
  it("tells assistive technology politely what is now shown and its place, and nothing as it first shows", () => {
    const { rerender } = render(shelf({ state: "loading" }));
    rerender(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 3 } }));
    const status = screen.getByRole("status");
    expect(status.textContent).toBe("");
    // A role="status" region is a polite live region.
    expect(status.getAttribute("aria-live")).toBeNull();

    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 3 } }));
    expect(screen.getByRole("status").textContent).toBe("Permission request, 2 of 3: Build the shell");
    rerender(shelf({ state: "question", requestId: "q3", questions: [{ id: "a", text: "Which branch?", multi_select: false }], place: { n: 3, m: 3 } }));
    expect(screen.getByRole("status").textContent).toBe("Question, 3 of 3: Which branch?");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R24
  it("says nothing when it comes back to a request after loading, as on a reconnect", () => {
    const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 3 } }));
    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 3 } }));
    const said = screen.getByRole("status").textContent;
    rerender(shelf({ state: "loading" }));
    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 3 } }));
    expect(screen.getByRole("status").textContent).toBe(said);
    expect(contents().hasAttribute("data-next")).toBe(false);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R24
  it("says the place when it moves on to a request it can't show", () => {
    const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 2 } }));
    rerender(shelf({ state: "unknown", place: { n: 2, m: 2 } }));
    expect(screen.getByRole("status").textContent).toBe("2 of 2: This session is waiting on a request Ersa can't show here.");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R10
  it("keeps focus on the shelf itself when the request whose answer held it is answered elsewhere", () => {
    const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 2 } }));
    act(() => screen.getByRole("button", { name: "Deny" }).focus());
    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 2 } }));
    expect(document.activeElement).toBe(screen.getByRole("dialog"));
  });

  // verifies: FLOW-APPROVE-PERMISSION-R10
  it("leaves focus the user took out of the shelf where it is as the shelf moves on", () => {
    const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 2 } }));
    const deny = screen.getByRole("button", { name: "Deny" });
    act(() => deny.focus());
    // Clicking text that takes no focus: focus leaves for nowhere.
    act(() => deny.blur());
    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 2 } }));
    expect(document.activeElement).toBe(document.body);
  });

  it("eases its height once per move, and leaves a running ease alone as it renders again", () => {
    const eases: { cancelled: boolean }[] = [];
    const [animate, box, style] = [HTMLElement.prototype.animate, HTMLElement.prototype.getBoundingClientRect, window.getComputedStyle];
    HTMLElement.prototype.animate = function () {
      const ease = { cancelled: false, onfinish: null, cancel: () => (ease.cancelled = true) };
      eases.push(ease);
      return ease as unknown as Animation;
    };
    let tall = false;
    HTMLElement.prototype.getBoundingClientRect = function () {
      return { height: tall ? 300 : 200 } as DOMRect;
    };
    window.getComputedStyle = (() => ({ getPropertyValue: (name: string) => (name === "--motion-settle" ? "260ms" : "ease") })) as unknown as typeof getComputedStyle;
    try {
      const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 2 } }));
      tall = true;
      rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 2 } }));
      expect(eases).toHaveLength(1);
      // Rendering the same request again, as the leaving copy goes, keeps the ease running.
      rerender(createElement(Shelf, { tile: { title: "Fix the build" }, shows: { state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 2 } }, live: false, onDecide: () => {} }));
      expect(eases).toHaveLength(1);
      expect(eases[0]?.cancelled).toBe(false);
    } finally {
      HTMLElement.prototype.animate = animate;
      HTMLElement.prototype.getBoundingClientRect = box;
      window.getComputedStyle = style;
    }
  });

  it("keeps the last request on screen, its answers unavailable, once it is settled, rather than say it is loading", () => {
    const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests") }));
    rerender(shelf({ state: "settled" }));
    const dialog = screen.getByRole("dialog");
    expect(dialog.textContent).toContain("Run the tests");
    expect(dialog.textContent).not.toContain("Loading");
    expect(dialog.textContent).not.toContain("Reconnecting");
    for (const name of ["Deny", "Allow"]) expect(screen.getByRole("button", { name })).toHaveProperty("disabled", true);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R24
  it("says the next request when it arrives after the last was settled, or after it couldn't be shown", () => {
    const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests") }));
    rerender(shelf({ state: "settled" }));
    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 2 } }));
    expect(screen.getByRole("status").textContent).toBe("Permission request, 2 of 2: Build the shell");
    expect(contents().hasAttribute("data-next")).toBe(true);

    rerender(shelf({ state: "loading" }));
    rerender(shelf({ state: "unknown", place: { n: 3, m: 4 } }));
    rerender(shelf({ state: "permission", request: request("r4", "Ship it"), place: { n: 4, m: 4 } }));
    expect(screen.getByRole("status").textContent).toBe("Permission request, 4 of 4: Ship it");
  });

  it("fades the last request's contents out over the next, out of reach, and not with reduced motion", () => {
    const { rerender } = render(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 3 } }));
    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 3 } }));
    const leaving = document.querySelector(".shelf-contents[data-leaving]") as HTMLElement;
    expect(leaving.textContent).toContain("Run the tests");
    expect(leaving.hasAttribute("inert")).toBe(true);
    expect(leaving.getAttribute("aria-hidden")).toBe("true");

    reduce(true);
    rerender(shelf({ state: "permission", request: request("r3", "Ship it"), place: { n: 3, m: 3 } }));
    expect(document.querySelector(".shelf-contents[data-leaving]")).toBeNull();
  });

  it("cross-fades its contents to the next request, but not as it first shows one", () => {
    const { rerender } = render(shelf({ state: "loading" }));
    rerender(shelf({ state: "permission", request: request("r1", "Run the tests"), place: { n: 1, m: 2 } }));
    expect(contents().hasAttribute("data-next")).toBe(false);
    rerender(shelf({ state: "permission", request: request("r2", "Build the shell"), place: { n: 2, m: 2 } }));
    expect(contents().hasAttribute("data-next")).toBe(true);

    const css = readFileSync(join(import.meta.dirname, "Shelf.css"), "utf8");
    expect(css).toMatch(/\.shelf-contents\[data-next\] \{\s+animation: shelf-next var\(--motion-quick\) var\(--motion-ease-out\);/);
    // With reduced motion on, the next request replaces the last at once.
    const reduced = css.slice(css.indexOf("@media (prefers-reduced-motion: reduce)"));
    expect(reduced).toMatch(/\.shelf-contents\[data-next\] \{\s+animation: none;/);
  });
});
