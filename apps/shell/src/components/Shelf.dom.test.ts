// @vitest-environment jsdom
// The shelf in a DOM as it moves from one request to the next: what assistive technology hears, and the contents'
// cross-fade (docs/design/RAIL.md, The shelf: Motion).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
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
