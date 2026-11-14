// @vitest-environment jsdom
// Tool cards and the thinking row in the transcript (issue #327, Test seams): a card opens and closes from its row by
// pointer or keyboard, says its tool, target, state and whether it is open, nests a subagent's calls inside the call
// that started them, and the thinking row opens to its summary.

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Envelope, Event } from "../protocol/agent-events.gen";
import { EVENT_VERSION, apply, emptyTranscript } from "../session/transcript";
import { Transcript } from "./Transcript";

afterEach(cleanup);

const at = (seconds: number) => new Date(Date.UTC(2026, 10, 16, 9, 0, seconds)).toISOString();

function transcriptOf(steps: Event[]) {
  return steps
    .map(
      (event, seq): Envelope => ({
        v: EVENT_VERSION,
        session_id: "s1",
        seq,
        ts: at(seq),
        source: { backend: "claude-code", backend_version: "2.1.316", input: "live" },
        event,
      }),
    )
    .reduce(apply, emptyTranscript);
}

const show = (steps: Event[], workspace?: string) =>
  render(
    createElement(Transcript, {
      title: "Tools",
      view: { id: "s1", transcript: transcriptOf(steps), stream: { state: "live" as const } },
      workspace,
      onDecide: () => {},
      onAnswer: () => {},
    }),
  ).container;

const read: Event = {
  type: "tool.requested",
  tool_use_id: "r1",
  name: "Read",
  input: { file_path: "D:\\Projects\\ersa\\docs\\GLOSSARY.md", limit: 40 },
  target: "D:\\Projects\\ersa\\docs\\GLOSSARY.md",
};
const readDone: Event = { type: "tool.result", tool_use_id: "r1", content: "# Glossary", is_error: false, duration_ms: 2400 };

/** The open part of the card whose row is `row`. */
const openPart = (row: HTMLElement) => {
  const card = row.closest(".tool-card");
  const part = card?.querySelector<HTMLElement>(":scope > .tool-open");
  if (!part) throw new Error("no open part");
  return part;
};

describe("a tool card", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R06
  // verifies: FLOW-READ-TRANSCRIPT-R08
  it("is one row with its state, the tool, its target relative to the workspace, and its duration", () => {
    const page = show([read, readDone], "D:/Projects/ersa");
    const row = screen.getByRole("button", { name: "Read docs\\GLOSSARY.md, Done" });
    expect(row.querySelector(".tool-name")?.textContent).toBe("Read");
    expect(row.querySelector(".tool-target")?.textContent).toBe("docs\\GLOSSARY.md");
    expect(row.querySelector(".tool-target")?.hasAttribute("data-path")).toBe(true);
    expect(row.querySelector(".tool-detail")?.textContent).toBe("2s");
    expect(row.querySelector(".tool-mark")?.getAttribute("data-state")).toBe("done");
    expect(page.querySelectorAll(".tool-card")).toHaveLength(1);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("says a failed call failed in its detail", () => {
    show([read, { type: "tool.result", tool_use_id: "r1", content: "No such file", is_error: true }]);
    const row = screen.getByRole("button", { name: /^Read .*, Failed$/ });
    expect(row.querySelector(".tool-detail")?.textContent).toBe("Failed");
    expect(row.querySelector(".tool-detail")?.hasAttribute("data-failed")).toBe(true);
    expect(row.querySelector(".tool-mark")?.getAttribute("data-state")).toBe("failed");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("says a call waits for the user while its permission request is open", () => {
    show([
      { type: "tool.requested", tool_use_id: "b1", name: "Bash", input: { command: "rm -rf build" }, target: "rm -rf build" },
      {
        type: "permission.requested",
        request_id: "p1",
        tool_use_id: "b1",
        tool_name: "Bash",
        input: { command: "rm -rf build" },
        suggestions: [],
        flags: { default_to_no: false, suppress_always_allow: false, requires_user_interaction: false, classifier_approvable: false },
      },
    ]);
    const row = screen.getByRole("button", { name: "Bash rm -rf build, Waiting for you" });
    expect(row.querySelector(".tool-mark")?.getAttribute("data-state")).toBe("waiting");
    expect(row.querySelector(".tool-target")?.hasAttribute("data-path")).toBe(false);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R06
  it("shows a running call's latest progress", () => {
    show([read, { type: "tool.progress", tool_use_id: "r1", detail: "Reading GLOSSARY.md" }]);
    const row = screen.getByRole("button", { name: /^Read .*, Running$/ });
    expect(row.querySelector(".tool-detail")?.textContent).toBe("Reading GLOSSARY.md");
    expect(row.querySelector(".tool-mark")?.getAttribute("data-state")).toBe("running");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R07
  // verifies: FLOW-READ-TRANSCRIPT-R12
  it("is closed at first, and its row opens and closes it to show the input and output", () => {
    show([read, readDone], "D:\\Projects\\ersa");
    const row = screen.getByRole("button", { name: /^Read docs/ });
    expect(row.getAttribute("aria-expanded")).toBe("false");
    expect(openPart(row).hasAttribute("inert")).toBe(true);

    fireEvent.click(row);
    expect(row.getAttribute("aria-expanded")).toBe("true");
    const part = openPart(row);
    expect(part.hasAttribute("inert")).toBe(false);
    const labels = [...part.querySelectorAll(".tool-label")].map((l) => l.textContent);
    expect(labels).toEqual(["Input", "Output"]);
    const fields = [...part.querySelectorAll(".tool-field")].map((f) => [f.querySelector("dt")?.textContent, f.querySelector("dd")?.textContent]);
    expect(fields).toEqual([
      ["file_path", "D:\\Projects\\ersa\\docs\\GLOSSARY.md"],
      ["limit", "40"],
    ]);
    expect(part.querySelector(".tool-output")?.textContent).toBe("# Glossary");

    fireEvent.click(row);
    expect(row.getAttribute("aria-expanded")).toBe("false");
    expect(openPart(row).hasAttribute("inert")).toBe(true);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R07
  it("shows a failed call's error under Error", () => {
    show([read, { type: "tool.result", tool_use_id: "r1", content: "No such file", is_error: true }]);
    const row = screen.getByRole("button", { name: /^Read / });
    fireEvent.click(row);
    const labels = [...openPart(row).querySelectorAll(".tool-label")].map((l) => l.textContent);
    expect(labels).toEqual(["Input", "Error"]);
    expect(openPart(row).querySelector(".tool-output")?.textContent).toBe("No such file");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R12
  it("is reached by keyboard as a button, and opens with Enter or Space as a button does", () => {
    show([read, readDone]);
    const row = screen.getByRole("button", { name: /^Read / });
    // A native button: in the tab order, and the browser turns Enter and Space into a click.
    expect(row.tagName).toBe("BUTTON");
    expect(row.getAttribute("type")).toBe("button");
    expect(row.tabIndex).toBe(0);
    row.focus();
    expect(document.activeElement).toBe(row);
  });

  it("names a call with no target by its tool and state", () => {
    show([{ type: "tool.requested", tool_use_id: "m1", name: "mcp__docs__guide", input: {} }]);
    expect(screen.getByRole("button", { name: "mcp__docs__guide, Running" })).toBeTruthy();
  });
});

describe("a subagent's calls", () => {
  const agent: Event = { type: "tool.requested", tool_use_id: "a1", name: "Agent", input: { description: "Count files", prompt: "Count them." }, target: "Count files" };
  const glob: Event = { type: "tool.requested", tool_use_id: "g1", name: "Glob", input: { pattern: "*" }, parent_tool_use_id: "a1", target: "*" };
  const globDone: Event = { type: "tool.result", tool_use_id: "g1", content: "a.txt", is_error: false };
  const agentDone: Event = { type: "tool.result", tool_use_id: "a1", content: "One file.", is_error: false };

  // verifies: FLOW-READ-TRANSCRIPT-R10
  it("sit inside the card of the call that started the subagent, above its output, not as blocks of the turn", () => {
    const page = show([agent, glob, globDone, agentDone]);
    const page_ = page.querySelector(".transcript-page");
    // One block of the turn: the Agent card. The Glob card is inside it.
    expect([...(page_?.children ?? [])].filter((c) => c.classList.contains("tool-card"))).toHaveLength(1);
    const parent = screen.getByRole("button", { name: "Agent Count files, Done" });
    const part = openPart(parent);
    const child = within(part).getByRole("button", { name: "Glob *, Done", hidden: true });
    expect(child.closest(".tool-calls")).toBeTruthy();
    // Above the output.
    const order = [...part.querySelectorAll(":scope > .tool-open-inner > .tool-open-body > :is(.tool-calls, .tool-output)")].map((e) => e.className.split(" ")[0]);
    expect(order).toEqual(["tool-calls", "tool-output"]);
  });

  it("show as blocks of the turn when the call that started them isn't in the transcript", () => {
    const page = show([glob]);
    expect(page.querySelectorAll(".transcript-page > .tool-card")).toHaveLength(1);
  });

  it("show as blocks of the turn when their parents loop, rather than being lost", () => {
    const page = show([
      { type: "tool.requested", tool_use_id: "x", name: "Glob", input: {}, parent_tool_use_id: "y" },
      { type: "tool.requested", tool_use_id: "y", name: "Grep", input: {}, parent_tool_use_id: "x" },
      { type: "tool.requested", tool_use_id: "z", name: "Read", input: {}, parent_tool_use_id: "z" },
    ]);
    expect(page.querySelectorAll(".transcript-page > .tool-card")).toHaveLength(3);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R07
  it("says Input and Error for a failed call with no input fields and no output", () => {
    show([{ type: "tool.requested", tool_use_id: "e", name: "Ping", input: {} }, { type: "tool.result", tool_use_id: "e", content: "", is_error: true }]);
    const row = screen.getByRole("button", { name: "Ping, Failed" });
    expect([...openPart(row).querySelectorAll(".tool-label")].map((l) => l.textContent)).toEqual(["Input", "Error"]);
    // Each says it is empty rather than showing an empty box.
    expect([...openPart(row).querySelectorAll(".tool-empty")].map((l) => l.textContent)).toEqual(["No input", "No output"]);
    expect(openPart(row).querySelector(".tool-output")).toBeNull();
  });
});

describe("the thinking row", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R11
  it("says the agent is thinking, then that it thought", () => {
    const thinking = show([{ type: "assistant.thinking", block_id: "m/0", final: false }]);
    expect(thinking.querySelector(".thinking-row")?.textContent).toBe("Thinking…");
    expect(thinking.querySelector(".thinking-row .tool-mark")?.getAttribute("data-state")).toBe("running");
    cleanup();
    const thought = show([{ type: "assistant.thinking", block_id: "m/0", final: true }]);
    expect(thought.querySelector(".thinking-row")?.textContent).toBe("Thought");
    // No summary, nothing to open.
    expect(thought.querySelector(".thinking-row button")).toBeNull();
  });

  // verifies: FLOW-READ-TRANSCRIPT-R11
  // verifies: FLOW-READ-TRANSCRIPT-R12
  it("opens to its summary when the event carries one", () => {
    show([{ type: "assistant.thinking", block_id: "m/0", final: true, summary: "Checked the glossary first." }]);
    const row = screen.getByRole("button", { name: "Thought" });
    expect(row.getAttribute("aria-expanded")).toBe("false");
    const part = row.closest(".thinking-row")?.querySelector<HTMLElement>(".tool-open");
    expect(part?.hasAttribute("inert")).toBe(true);
    fireEvent.click(row);
    expect(row.getAttribute("aria-expanded")).toBe("true");
    expect(part?.hasAttribute("inert")).toBe(false);
    expect(part?.textContent).toBe("Checked the glossary first.");
  });
});
