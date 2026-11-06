import { readFileSync } from "node:fs";
import { join } from "node:path";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { PermissionRequested } from "../protocol/agent-events.gen";
import type { ShelfShows } from "../session/shelf";
import { Shelf } from "./Shelf";
import { PermissionInput } from "./Transcript";

const flags = { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false };
const request: PermissionRequested = {
  type: "permission.requested",
  request_id: "r1",
  tool_use_id: "t1",
  tool_name: "Bash",
  display_name: "Run a command",
  description: "Run the reconnect tests",
  input: { command: "pnpm --filter @ersa/shell test reconnect", description: "Run the reconnect tests" },
  blocked_path: "C:\\Users\\me\\ersa\\target",
  suggestions: [],
  flags,
  reason: { kind: "rule", text: "Commands need approval in this project." },
};

const draw = (shows: ShelfShows, more: { ring?: number; cost?: string; onRetry?: () => void } = {}) =>
  renderToStaticMarkup(createElement(Shelf, { title: "Fix the flaky reconnect test", shows, ...more }));

/** The text of `html` in reading order, its tags dropped. */
const text = (html: string) => html.replace(/<[^>]+>/g, " ").replaceAll("&#x27;", "'").replace(/\s+/g, " ").trim();

describe("the shelf", () => {
  // verifies: FLOW-APPROVE-PERMISSION-R11
  it("shows a permission request's kind, what is asked, its input as the transcript's card shows it, path, reason and readouts", () => {
    const html = draw({ state: "permission", request }, { ring: 0.53, cost: "$0.82" });
    expect(html).toContain('<p class="shelf-heading"><span>Permission request</span></p>');
    // What is asked: the description first.
    expect(html).toContain('<p class="shelf-asked">Run the reconnect tests</p>');
    // The input, drawn by the same component as the transcript's permission card.
    expect(html).toContain(renderToStaticMarkup(createElement(PermissionInput, { request })));
    expect(html).toContain("pnpm --filter @ersa/shell test reconnect");
    expect(html).toContain('Path: <span class="mono">C:\\Users\\me\\ersa\\target</span>');
    expect(html).toContain("Commands need approval in this project.");
    expect(html).toContain('aria-label="Context: 53 percent used"');
    expect(text(html)).toContain("53% · $0.82");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R11
  it("asks with the tool's display name without a description, else its name, and leaves out what the request lacks", () => {
    const bare = { ...request, description: undefined, blocked_path: undefined, reason: undefined, input: { command: "ls" } };
    expect(draw({ state: "permission", request: bare })).toContain('<p class="shelf-asked">Run a command</p>');
    const html = draw({ state: "permission", request: { ...bare, display_name: undefined } });
    expect(html).toContain('<p class="shelf-asked">Bash</p>');
    expect(html).not.toContain("Path:");
    expect(html).not.toContain("shelf-readouts");
    // Only the cost: no ring and no dot.
    expect(text(draw({ state: "permission", request: bare }, { cost: "$0.82" }))).toMatch(/ \$0\.82 Deny/);
  });

  // verifies: FLOW-APPROVE-PERMISSION-R23
  it("is a dialog that leaves the window usable, named for its session, read kind, asked, details, readouts, answers", () => {
    const html = draw({ state: "permission", request, place: { n: 1, m: 3 } }, { ring: 0.53, cost: "$0.82" });
    expect(html).toMatch(/^<div class="shelf" role="dialog" aria-label="Request from Fix the flaky reconnect test"/);
    expect(html).not.toContain("aria-modal");
    const read = text(html);
    const order = ["Permission request", "1 of 3", "Run the reconnect tests", "command", "Path:", "Commands need approval", "53%", "$0.82", "Deny", "Allow"];
    const at = order.map((part) => read.indexOf(part));
    expect(at.every((i) => i >= 0)).toBe(true);
    expect(at).toEqual([...at].sort((a, b) => a - b));
  });

  // verifies: FLOW-APPROVE-PERMISSION-R21
  it("says it is loading, and when reading failed, why, with Retry", () => {
    expect(draw({ state: "loading" })).toContain('<p class="shelf-note" role="status">Loading…</p>');
    const failed = draw({ state: "failed", message: "the session is gone" }, { onRetry: () => {} });
    expect(failed).toContain('role="alert"');
    expect(text(failed)).toBe("Ersa can't read this session's requests. the session is gone Retry");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R25
  it("says the session waits on a request it can't show, and offers no answers for it", () => {
    const html = draw({ state: "unknown", place: { n: 1, m: 2 } }, { cost: "$0.82" });
    expect(text(html)).toBe("1 of 2 This session is waiting on a request Ersa can't show here. $0.82");
    expect(html).not.toContain("<button");
    expect(html).toContain('data-tone="caution"');
  });

  it("draws Deny and Allow with the primary as the request says, and its suggestions unless it suppresses them", () => {
    const suggestions = [{ kind: "allow_rule" as const, label: "Allow pnpm commands in this project for the session", payload: {} }];
    const html = draw({ state: "permission", request: { ...request, suggestions } });
    expect(html).toContain("Allow pnpm commands in this project for the session");
    expect(html).toMatch(/data-variant="quiet" class="button">Deny<\/button><button type="button" data-variant="primary" class="button">Allow/);
    const no = draw({ state: "permission", request: { ...request, suggestions, flags: { ...flags, default_to_no: true, suppress_always_allow: true } } });
    expect(no).not.toContain("Allow pnpm commands");
    expect(no).toMatch(/data-variant="primary" class="button">Deny<\/button><button type="button" data-variant="quiet" class="button">Allow/);
  });

  it("is the accent for a question, and shows what it asks", () => {
    const html = draw({ state: "question", requestId: "r3", questions: [{ id: "q1", text: "Which branch?", multi_select: false }] });
    expect(html).toContain('data-tone="accent"');
    expect(text(html)).toBe("Question Which branch?");
  });

  // verifies: FLOW-APPROVE-PERMISSION-R04
  it("lies over the transcript, placed beside its tile, so the transcript never moves, resizes or scrolls for it", () => {
    const css = readFileSync(join(import.meta.dirname, "Shelf.css"), "utf8");
    const rule = css.slice(css.indexOf(".shelf {"), css.indexOf("}", css.indexOf(".shelf {")));
    // Out of the page's flow, so it takes no room from anything, and above the transcript.
    expect(rule).toContain("position: fixed;");
    expect(rule).toContain("position-anchor: --open-tile;");
    expect(rule).toContain("z-index: var(--stack-shelf);");
    expect(rule).toContain("width: var(--size-shelf);");
  });
});
