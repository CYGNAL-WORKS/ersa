// @vitest-environment jsdom
// The agent's words as formatted text (FLOW-READ-TRANSCRIPT): Markdown with GitHub's extensions, never HTML; images
// never loaded; links opened only for web and email addresses; Copy on code blocks; and text that streams in pieces.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Envelope, Event } from "../protocol/agent-events.gen";
import { EVENT_VERSION, apply, emptyTranscript } from "../session/transcript";
import { AgentText } from "./AgentText";
import { Transcript } from "./Transcript";

afterEach(cleanup);

const show = (text: string, onOpenLink?: (url: string) => void) => render(createElement(AgentText, { text, onOpenLink })).container;

/** The transcript of one agent message, as the window builds it from events. */
function transcriptOf(events: Event[]) {
  return events
    .map(
      (event, seq): Envelope => ({
        v: EVENT_VERSION,
        session_id: "s1",
        seq,
        ts: "2026-11-13T09:00:00Z",
        source: { backend: "claude-code", backend_version: "2.1.302", input: "live" },
        event,
      }),
    )
    .reduce(apply, emptyTranscript);
}

const transcriptProps = (text: string, onOpenLink?: (url: string) => void) => ({
  title: "Read the links",
  view: { id: "s1", transcript: transcriptOf([{ type: "assistant.text", block_id: "m1/0", text, final: true }]), stream: { state: "live" as const } },
  onDecide: () => {},
  onAnswer: () => {},
  onOpenLink,
});

describe("Markdown with GitHub's extensions", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R01
  it("renders tables, strikethrough, task lists and autolinks as elements", () => {
    const page = show(
      [
        "## Plan",
        "",
        "Some **bold**, *italic*, ~~gone~~ and `code`.",
        "",
        "| Name | Size |",
        "| --- | --- |",
        "| a.ts | 12 |",
        "",
        "- [x] Done",
        "- [ ] Todo",
        "",
        "1. First",
        "",
        "> Quoted",
        "",
        "---",
        "",
        "See https://example.com/docs and someone@example.com.",
      ].join("\n"),
    );
    expect(page.querySelector("h2")?.textContent).toBe("Plan");
    expect(page.querySelector("strong")?.textContent).toBe("bold");
    expect(page.querySelector("em")?.textContent).toBe("italic");
    expect(page.querySelector("del")?.textContent).toBe("gone");
    expect(page.querySelector("p code")?.textContent).toBe("code");
    expect([...page.querySelectorAll("th")].map((th) => th.textContent)).toEqual(["Name", "Size"]);
    expect([...page.querySelectorAll("td")].map((td) => td.textContent)).toEqual(["a.ts", "12"]);
    const boxes = screen.getAllByRole("checkbox");
    expect(boxes.map((box) => box.getAttribute("aria-checked"))).toEqual(["true", "false"]);
    // A task item's box can't be changed.
    expect(boxes.every((box) => box.getAttribute("aria-disabled") === "true")).toBe(true);
    expect(page.querySelector("ol li")?.textContent).toBe("First");
    expect(page.querySelector("blockquote")?.textContent?.trim()).toBe("Quoted");
    expect(page.querySelector("hr")).not.toBeNull();
    expect(screen.getByRole("link", { name: "https://example.com/docs" }).getAttribute("href")).toBe("https://example.com/docs");
    expect(screen.getByRole("link", { name: "someone@example.com" }).getAttribute("href")).toBe("mailto:someone@example.com");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R01
  it("renders the agent's text in the transcript as formatted text", () => {
    render(createElement(Transcript, transcriptProps("A **bold** word.")));
    expect(document.querySelector(".agent-text strong")?.textContent).toBe("bold");
  });
});

describe("what Ersa does not show as itself", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R02
  it("shows raw HTML as the text it is", () => {
    const page = show('Before <b onclick="x()">bold</b> after.\n\n<script>alert(1)</script>\n\n<img src="https://example.com/a.png">');
    expect(page.querySelector("b, script, img")).toBeNull();
    expect(page.textContent).toContain('<b onclick="x()">bold</b>');
    expect(page.textContent).toContain("<script>alert(1)</script>");
    expect(page.textContent).toContain('<img src="https://example.com/a.png">');
  });

  // verifies: FLOW-READ-TRANSCRIPT-R02
  it("shows an image as its alternative text linked to its address, never loaded", () => {
    const page = show("![The build graph](https://example.com/graph.png) and ![](https://example.com/bare.png)");
    expect(page.querySelector("img")).toBeNull();
    expect(screen.getByRole("link", { name: "The build graph" }).getAttribute("href")).toBe("https://example.com/graph.png");
    // With no alternative text, its address is the text.
    expect(screen.getByRole("link", { name: "https://example.com/bare.png" }).getAttribute("href")).toBe("https://example.com/bare.png");
  });
});

describe("links", () => {
  // verifies: FLOW-READ-TRANSCRIPT-R03
  it("opens web and email addresses through the Shell, and never in the window", () => {
    const opened = vi.fn();
    render(
      createElement(
        Transcript,
        transcriptProps("[web](http://example.com/a) [secure](https://example.com/b) [mail](mailto:someone@example.com)", opened),
      ),
    );
    for (const name of ["web", "secure", "mail"]) {
      const link = screen.getByRole("link", { name });
      // fireEvent returns false when the default action, following the link in the window, was prevented.
      expect(fireEvent.click(link)).toBe(false);
    }
    expect(opened.mock.calls).toEqual([["http://example.com/a"], ["https://example.com/b"], ["mailto:someone@example.com"]]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R03
  it("shows a link with any other scheme as text that opens nothing", () => {
    const opened = vi.fn();
    render(
      createElement(
        Transcript,
        transcriptProps(
          "[script](javascript:alert(1)) [file](file:///C:/Windows/notepad.exe) [chat](irc://irc.example.com/ersa) [ftp](ftp://example.com/) [relative](notes.md) ![pic](file:///C:/pic.png)",
          opened,
        ),
      ),
    );
    expect(screen.queryAllByRole("link")).toEqual([]);
    const text = document.querySelector(".agent-text");
    for (const word of ["script", "file", "chat", "ftp", "relative", "pic"]) {
      fireEvent.click(screen.getByText(word, { exact: false }));
      expect(text?.textContent).toContain(word);
    }
    expect(opened).not.toHaveBeenCalled();
  });
});

describe("Copy on a code block", () => {
  let written: string[];
  beforeEach(() => {
    written = [];
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: (text: string) => (written.push(text), Promise.resolve()) },
    });
  });

  // verifies: FLOW-READ-TRANSCRIPT-R04
  it("puts the block's text on the clipboard exactly, without its fences or language", async () => {
    show(["Run:", "", "```bash", "cargo test -p ersa-core", "  echo 'two  spaces'", "", "```", "", "And:", "", "    indented line"].join("\n"));
    const [fenced, indented] = screen.getAllByRole("button", { name: "Copy code" });
    expect(document.querySelector(".code-block-language")?.textContent).toBe("bash");
    await act(async () => {
      fireEvent.click(fenced!);
    });
    await act(async () => {
      fireEvent.click(indented!);
    });
    expect(written).toEqual(["cargo test -p ersa-core\n  echo 'two  spaces'\n", "indented line"]);
  });

  // verifies: FLOW-READ-TRANSCRIPT-R04
  it("says Copied once copied, until the pointer leaves it", async () => {
    show("```\nx\n```");
    const copy = screen.getByRole("button", { name: "Copy code" });
    await act(async () => {
      fireEvent.click(copy);
    });
    expect(copy.getAttribute("aria-label")).toBe("Copied");
    expect(copy.getAttribute("title")).toBe("Copied");
    fireEvent.pointerLeave(copy);
    expect(copy.getAttribute("aria-label")).toBe("Copy code");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R04
  it("names no language when the block has none", () => {
    show("```\nplain\n```");
    expect(document.querySelector(".code-block-language")?.textContent).toBe("");
  });
});

describe("text that streams", () => {
  /** Renders `text` growing piece by piece in one AgentText, as a streaming message re-renders. */
  function stream(pieces: string[]) {
    let text = "";
    const view = render(createElement(AgentText, { text }));
    return {
      next() {
        text += pieces.shift();
        view.rerender(createElement(AgentText, { text }));
        return view.container;
      },
    };
  }

  // verifies: FLOW-READ-TRANSCRIPT-R05
  it("renders an open code block as far as it has arrived, and the text before it stays", () => {
    const s = stream(["Here is the fix.\n\n", "```rust\nfn main() {\n", "    run();\n", "}\n```\n"]);
    const before = s.next().querySelector("p");
    expect(before?.textContent).toBe("Here is the fix.");
    let page = s.next();
    expect(page.querySelector("pre code")?.textContent).toBe("fn main() {");
    expect(page.querySelector(".code-block-language")?.textContent).toBe("rust");
    page = s.next();
    expect(page.querySelector("pre code")?.textContent).toBe("fn main() {\n    run();");
    page = s.next();
    expect(page.querySelector("pre code")?.textContent).toBe("fn main() {\n    run();\n}");
    // The same element, unchanged: nothing before the block was drawn again.
    expect(page.querySelector("p")).toBe(before);
    expect(before?.textContent).toBe("Here is the fix.");
  });

  // verifies: FLOW-READ-TRANSCRIPT-R05
  it("renders an open list and table as far as they have arrived", () => {
    const s = stream(["Steps:\n\n", "- one\n", "- two\n", "\n| a | b |\n| --- | --- |\n", "| 1 | 2 |\n", "| 3 |"]);
    const before = s.next().querySelector("p");
    expect([...s.next().querySelectorAll("li")].map((li) => li.textContent)).toEqual(["one"]);
    expect([...s.next().querySelectorAll("li")].map((li) => li.textContent)).toEqual(["one", "two"]);
    let page = s.next();
    expect([...page.querySelectorAll("th")].map((th) => th.textContent)).toEqual(["a", "b"]);
    page = s.next();
    expect([...page.querySelectorAll("td")].map((td) => td.textContent)).toEqual(["1", "2"]);
    page = s.next();
    expect([...page.querySelectorAll("tbody tr")].map((tr) => tr.textContent)).toEqual(["12", "3"]);
    expect(page.querySelector("p")).toBe(before);
    expect([...page.querySelectorAll("li")].map((li) => li.textContent)).toEqual(["one", "two"]);
  });
});
