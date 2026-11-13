// @vitest-environment jsdom
// A user's message with its attachments in the transcript (FLOW-SEND-MESSAGE-R11; DESIGN.md, Layout), rendered from
// the replay of a recorded session (2.1.316/07-attachments): a message with an image, a file and a folder, then one of
// a file alone.

import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it } from "vitest";
import type { Envelope } from "../protocol/agent-events.gen";
import { events } from "../session/replays";
import { EVENT_VERSION, type MessageAttachment, type Transcript as Shown, apply, emptyTranscript } from "../session/transcript";
import { Transcript } from "./Transcript";

afterEach(cleanup);

const draw = (transcript: Shown) =>
  render(createElement(Transcript, { title: "Attachments", view: { id: "s1", transcript, stream: { state: "live" } }, onDecide: () => {}, onAnswer: () => {} }));

const replayed = () => events("2.1.316-07-attachments").reduce(apply, emptyTranscript);

const message = (attachments: MessageAttachment[]): Shown => {
  const envelope: Envelope = {
    v: EVENT_VERSION,
    session_id: "s1",
    seq: 0,
    ts: "2026-11-06T09:00:00Z",
    source: { backend: "claude-code", backend_version: "2.1.316", input: "live" },
    event: { type: "user.message", text: "Look", origin: "human", attachments },
  };
  return apply(emptyTranscript, envelope);
};

describe("a message's attachments in the transcript", () => {
  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows an image as the image and a file or folder by its name, above the words", async () => {
    draw(replayed());
    await act(async () => {});
    const first = screen.getAllByRole("group", { name: "You" })[0] as HTMLElement;
    const list = within(first).getByRole("list", { name: "Attachments" });
    const image = within(list).getByRole("img", { name: "Attached image" }) as HTMLImageElement;
    expect(image.src).toMatch(/^data:image\/png;base64,iVBOR/);
    // A file or folder by its path's last part, its accessible name the path the agent was given.
    expect(within(list).getAllByRole("button").map((b) => [b.textContent, b.getAttribute("aria-label")])).toEqual([
      ["todo.md", "notes/todo.md"],
      ["src", "src"],
    ]);
    // Above the words.
    const words = within(first).getByText(/What colour is the attached image/);
    expect(list.compareDocumentPosition(words) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("lets a keyboard user reach a file's path: focus moves to it, it names itself by the path, and the tooltip shows it", async () => {
    draw(replayed());
    await act(async () => {});
    const chip = screen.getByRole("button", { name: "notes/todo.md" });
    chip.focus();
    expect(document.activeElement).toBe(chip);
    expect(chip.tabIndex).toBe(0);
    const tip = chip.nextElementSibling as HTMLElement;
    expect(tip.className).toBe("attachment-tip");
    expect(tip.textContent).toBe("notes/todo.md");
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows the path after a press on touch, until the next press anywhere", async () => {
    draw(replayed());
    await act(async () => {});
    const chip = screen.getByRole("button", { name: "notes/todo.md" });
    fireEvent.pointerDown(chip, { pointerType: "touch" });
    expect(chip.hasAttribute("data-tip")).toBe(true);
    fireEvent.pointerDown(document.body, { pointerType: "touch" });
    expect(chip.hasAttribute("data-tip")).toBe(false);
    // A mouse press leaves it to the resting pointer.
    fireEvent.pointerDown(chip, { pointerType: "mouse" });
    expect(chip.hasAttribute("data-tip")).toBe(false);
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows a message of attachments alone as its attachments, with no empty line of words", async () => {
    draw(replayed());
    await act(async () => {});
    const humans = screen.getAllByRole("group", { name: "You" }).filter((b) => within(b).queryByRole("list", { name: "Attachments" }));
    const alone = humans.at(-1) as HTMLElement;
    expect(within(alone).getByRole("button").getAttribute("aria-label")).toBe("README.md");
    expect(alone.querySelector(".transcript-text")).toBeNull();
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows an image the window can't show, or one that fails to load, as the attachment named as an image", async () => {
    draw(message([{ kind: "image", media_type: "text/html", data: "PGI+" }]));
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByRole("listitem").textContent).toBe("Attached image");
    cleanup();
    draw(message([{ kind: "image", media_type: "image/png", data: "broken" }]));
    fireEvent.error(screen.getByRole("img", { name: "Attached image" }));
    expect(screen.queryByRole("img", { name: "Attached image" })).toBeNull();
    expect(screen.getByRole("listitem").textContent).toBe("Attached image");
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("keeps an image's place when it fails to draw after its place is set, the icon and its name in it", () => {
    const header = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 2, 128, 0, 0, 1, 224, 8, 6, 0, 0, 0];
    draw(message([{ kind: "image", media_type: "image/png", data: btoa(String.fromCharCode(...header)) }]));
    const place = document.querySelector(".attachment-image") as HTMLElement;
    fireEvent.error(screen.getByRole("img", { name: "Attached image" }));
    expect(document.querySelector(".attachment-image")).toBe(place);
    expect(place.style.getPropertyValue("--image-width")).toBe("640px");
    expect(place.textContent).toBe("Attached image");
    expect(place.querySelector("svg")).not.toBeNull();
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("fades the image in once it is drawn", () => {
    draw(message([{ kind: "image", media_type: "image/png", data: "iVBOR" }]));
    const place = document.querySelector(".attachment-image") as HTMLElement;
    expect(place.dataset.state).toBe("drawing");
    fireEvent.load(screen.getByRole("img", { name: "Attached image" }));
    expect(place.dataset.state).toBe("drawn");
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("shows the bubble at once, its image's box already at the size its header states, before the image draws", () => {
    // A PNG header stating 640 by 480, and no image data after it: the image never draws.
    const header = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52, 0, 0, 2, 128, 0, 0, 1, 224, 8, 6, 0, 0, 0];
    draw(message([{ kind: "image", media_type: "image/png", data: btoa(String.fromCharCode(...header)) }]));
    const bubble = screen.getByRole("group", { name: "You" });
    expect(bubble.hasAttribute("data-decoding")).toBe(false);
    const place = document.querySelector(".attachment-image") as HTMLElement;
    expect(place.hasAttribute("data-sized")).toBe(true);
    expect(place.style.getPropertyValue("--image-width")).toBe("640px");
    expect(place.style.getPropertyValue("--image-ratio")).toBe("640 / 480");
    expect(Number(place.style.getPropertyValue("--image-aspect"))).toBeCloseTo(640 / 480);
  });
});
