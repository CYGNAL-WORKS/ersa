// @vitest-environment jsdom
// The composer's attachments in a DOM (docs/design/COMPOSER.md, Attachments and The attach menu; FLOW-SEND-MESSAGE):
// the attach menu, what each attachment shows, removing one, a refused one, and the status line while a message with
// attachments is on its way or Ersa can't tell where it went.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Attached } from "../session/attachments";
import type { Sending } from "../session/follow";
import type { AttachWhat } from "./AttachMenu";
import { Composer } from "./Composer";

afterEach(cleanup);

const notes: Attached = { path: "D:\\work\\repo\\notes.md", name: "notes.md", kind: "file", detail: "212 lines", copied: { workspace: "w-1", copied: false } };
const shot: Attached = { path: "C:\\pics\\shot.png", name: "shot.png", kind: "image", detail: "84 KB" };
const src: Attached = { path: "D:\\work\\repo\\src", name: "src", kind: "folder", detail: "Folder", copied: { workspace: "w-1", copied: false } };
// Inside the folder by its path, but Core resolved it elsewhere, as through a junction.
const vendored: Attached = { path: "D:\\work\\repo\\vendor\\key.txt", name: "key.txt", kind: "file", detail: "1 line", copied: { workspace: "w-1", copied: true } };

type Props = {
  sending?: Sending;
  workspace?: string;
  initialAttachments?: Attached[];
  initialText?: string;
  onAttach?: (what: AttachWhat) => Promise<Attached[]>;
  onSend?: (text: string, attachments: readonly Attached[]) => Promise<boolean>;
};
const draw = (props: Props = {}) =>
  createElement(Composer, { autoFocus: false, onSend: async () => true, workspace: "w-1", ...props });
const words = () => screen.getByRole("textbox", { name: "Message" }) as HTMLTextAreaElement;
const attachButton = () => screen.getByRole("button", { name: "Attach" });
const send = () => screen.getByRole("button", { name: "Send" });
const shown = () => Array.from(document.querySelectorAll(".composer-attachment")).map((li) => li.textContent);
const statusLine = () => document.querySelector(".composer-status-line");

describe("the attach menu", () => {
  // verifies: FLOW-SEND-MESSAGE-R07
  it("offers files, images and a folder, each through its dialog, and says attachments can be pasted or dropped", async () => {
    const onAttach = vi.fn(async () => [] as Attached[]);
    render(draw({ onAttach }));
    fireEvent.click(attachButton());
    const menu = screen.getByRole("menu", { name: "Attach" });
    expect(screen.getAllByRole("menuitem").map((m) => m.textContent)).toEqual(["Add files…", "Add images…", "Add a folder…"]);
    expect(menu.textContent).toContain("You can also paste attachments, or drop them on the box.");
    // Opening moves focus to the first choice; the arrows move among them.
    expect(document.activeElement?.textContent).toBe("Add files…");
    fireEvent.keyDown(document.activeElement as Element, { key: "ArrowDown" });
    await act(async () => {
      fireEvent.keyDown(document.activeElement as Element, { key: "Enter" });
    });
    expect(onAttach).toHaveBeenCalledWith("images");
    // Choosing closes the menu with focus back on attach.
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(attachButton());
    fireEvent.click(attachButton());
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Add a folder…" }));
    });
    expect(onAttach).toHaveBeenLastCalledWith("folder");
  });

  // verifies: FLOW-SEND-MESSAGE-R07
  it("closes with Escape, focus back on attach, and is unavailable where nothing can be attached", () => {
    render(draw({ onAttach: async () => [] }));
    fireEvent.click(attachButton());
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(attachButton());
    cleanup();
    render(draw());
    expect((attachButton() as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("attachments", () => {
  // verifies: FLOW-SEND-MESSAGE-R09
  it("each show their name and detail above the words", () => {
    render(draw({ initialAttachments: [notes, src] }));
    const list = screen.getByRole("list", { name: "Attachments" });
    expect(list.compareDocumentPosition(words()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(shown()).toEqual(["notes.md212 lines", "srcFolder"]);
  });

  // verifies: FLOW-SEND-MESSAGE-R09
  it("can each be removed, focus moving to the next one's remove, else to the words", () => {
    render(draw({ initialAttachments: [notes, shot, src] }));
    fireEvent.click(screen.getByRole("button", { name: "Remove shot.png" }));
    expect(shown()).toEqual(["notes.md212 lines", "srcFolder"]);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Remove src" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove src" }));
    expect(document.activeElement).toBe(words());
    fireEvent.click(screen.getByRole("button", { name: "Remove notes.md" }));
    expect(screen.queryByRole("list", { name: "Attachments" })).toBeNull();
  });

  // verifies: FLOW-SEND-MESSAGE-R12
  it("holds the same file or folder once, however often it is added", async () => {
    render(draw({ initialAttachments: [notes], onAttach: async () => [{ ...notes, path: "d:/work/repo/NOTES.md" }, src, src] }));
    fireEvent.click(attachButton());
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: "Add files…" }));
    });
    expect(shown()).toEqual(["notes.md212 lines", "srcFolder"]);
  });

  // verifies: FLOW-SEND-MESSAGE-R13
  it("go with the message, alone if there are no words, and clear once it is accepted", async () => {
    const onSend = vi.fn(async (_text: string, _attachments: readonly Attached[]) => true);
    render(draw({ initialAttachments: [notes, shot], onSend }));
    expect(send().getAttribute("aria-disabled")).toBeNull();
    await act(async () => {
      fireEvent.click(send());
    });
    expect(onSend).toHaveBeenCalledWith("", [notes, shot]);
    expect(shown()).toEqual([]);
  });

  // verifies: FLOW-SEND-MESSAGE-R14
  it("say they will be copied into its .ersa folder when Core says sending copies them, and an image never", () => {
    render(draw({ initialAttachments: [notes, vendored, shot] }));
    expect(shown()).toEqual(["notes.md212 lines", "key.txt1 lineWill be copied into the workspace's .ersa folder", "shot.png84 KB"]);
    cleanup();
    // A workspace not made yet holds nothing: every file and folder is copied.
    render(draw({ initialAttachments: [notes, shot], workspace: undefined }));
    expect(shown()).toEqual(["notes.md212 linesWill be copied into the workspace's .ersa folder", "shot.png84 KB"]);
  });
});

describe("an attachment Ersa can't send", () => {
  // verifies: FLOW-SEND-MESSAGE-R10
  it("is marked itself with why, and nothing is sent until it is removed", async () => {
    let answer: (ok: boolean) => void = () => {};
    const onSend = vi.fn(() => new Promise<boolean>((r) => (answer = r)));
    const { rerender } = render(draw({ initialAttachments: [notes, shot], initialText: "Look", onSend }));
    await act(async () => {
      fireEvent.click(send());
    });
    answer(false);
    rerender(draw({ initialAttachments: [notes, shot], initialText: "Look", onSend, sending: { state: "failed", message: "This image is too large.", index: 1 } }));
    await act(async () => {});
    const marked = document.querySelector(".composer-attachment[data-problem]");
    expect(marked?.textContent).toBe("shot.pngThis image is too large.");
    expect(screen.getByRole("alert").textContent).toBe("This image is too large.");
    // Said beside the attachment, not in the status line.
    expect(statusLine()).toBeNull();
    expect(send().getAttribute("aria-disabled")).toBe("true");
    fireEvent.click(send());
    expect(onSend).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Remove shot.png" }));
    expect(send().getAttribute("aria-disabled")).toBeNull();
  });

  // verifies: FLOW-SEND-MESSAGE-R10
  it("says why in the status line when the attachment refused is no longer there to mark", () => {
    render(draw({ initialText: "Look", sending: { state: "failed", message: "This image is too large.", index: 3 } }));
    expect(statusLine()?.textContent).toBe("This image is too large.");
  });

  // verifies: FLOW-SEND-MESSAGE-R10
  it("holds the message when it is attached marked, as a path Ersa couldn't read", () => {
    render(draw({ initialAttachments: [{ ...notes, detail: "", problem: "This file no longer exists." }], initialText: "Look" }));
    expect(send().getAttribute("aria-disabled")).toBe("true");
  });
});

describe("the status line for a message with attachments", () => {
  // verifies: FLOW-SEND-MESSAGE-R15
  it("says Ersa can't tell whether it arrived, with the caution glyph, and keeps the message", () => {
    render(draw({ initialAttachments: [notes], initialText: "Look", sending: { state: "unknown" } }));
    expect(statusLine()?.textContent).toBe("Ersa can't tell whether your message reached the agent. If it did, the transcript shows it.");
    expect(statusLine()?.querySelector('.glyph[data-tone="caution"]')).not.toBeNull();
    expect(statusLine()?.textContent).not.toMatch(/fail/i);
    expect(words().value).toBe("Look");
    expect(shown()).toEqual(["notes.md212 lines"]);
  });

  // verifies: FLOW-SEND-MESSAGE-R16
  it("shows how far its attachments are copied, or that the backend is starting", () => {
    const { rerender } = render(draw({ sending: { state: "sending", progress: { phase: "copying", share: 0.254 } } }));
    expect(statusLine()?.textContent).toBe("Copying attachments, 25 percent done…");
    expect(statusLine()?.querySelector(".context-ring-used")?.getAttribute("stroke-dasharray")).toBe("25 100");
    rerender(draw({ sending: { state: "sending", progress: { phase: "starting" } } }));
    expect(statusLine()?.textContent).toBe("Starting the backend…");
    rerender(draw({ sending: { state: "sending" } }));
    expect(statusLine()).toBeNull();
  });
});
