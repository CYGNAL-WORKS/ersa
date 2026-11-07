// @vitest-environment jsdom
// A user's message with its attachments in the transcript (FLOW-SEND-MESSAGE-R11), rendered from the replay of a
// recorded session (2.1.316/07-attachments): a message with an image, a file and a folder, then one of a file alone.

import { cleanup, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Envelope } from "../protocol/agent-events.gen";
import { events } from "../session/replays";
import { EVENT_VERSION, type Transcript as Shown, apply, emptyTranscript } from "../session/transcript";
import { Transcript } from "./Transcript";

afterEach(cleanup);

const draw = (transcript: Shown) =>
  render(createElement(Transcript, { title: "Attachments", view: { id: "s1", transcript, stream: { state: "live" } }, onDecide: () => {} }));

const replayed = () => events("2.1.316-07-attachments").reduce(apply, emptyTranscript);

describe("a message's attachments in the transcript", () => {
  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows an image as the image and a file or folder by its name, above the words", () => {
    draw(replayed());
    const bubbles = screen.getAllByRole("group", { name: "You" });
    const first = bubbles[0] as HTMLElement;
    const list = within(first).getByRole("list", { name: "Attachments" });
    const image = within(list).getByRole("img", { name: "Attached image 1 of 1" }) as HTMLImageElement;
    expect(image.src).toMatch(/^data:image\/png;base64,iVBOR/);
    const named = within(list).getAllByRole("listitem").filter((li) => li.querySelector("img") === null);
    expect(named.map((li) => [li.textContent, li.getAttribute("title")])).toEqual([
      ["todo.md", "notes/todo.md"],
      ["src", "src"],
    ]);
    // Above the words.
    const words = within(first).getByText(/What colour is the attached image/);
    expect(list.compareDocumentPosition(words) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows a message of attachments alone as its attachments, with no empty line of words", () => {
    draw(replayed());
    const humans = screen.getAllByRole("group", { name: "You" }).filter((b) => within(b).queryByRole("list", { name: "Attachments" }));
    const alone = humans.at(-1) as HTMLElement;
    expect(within(alone).getByRole("listitem").textContent).toBe("README.md");
    expect(alone.querySelector(".transcript-text")).toBeNull();
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows an image of a kind the window does not show as one by name, still there", () => {
    const envelope: Envelope = {
      v: EVENT_VERSION,
      session_id: "s1",
      seq: 0,
      ts: "2026-11-06T09:00:00Z",
      source: { backend: "claude-code", backend_version: "2.1.316", input: "live" },
      event: { type: "user.message", text: "Look", origin: "human", attachments: [{ kind: "image", media_type: "text/html", data: "PGI+" }] },
    };
    draw(apply(emptyTranscript, envelope));
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByRole("listitem").textContent).toBe("Attached image 1 of 1");
  });
});
