import { describe, expect, it } from "vitest";
import type { AttachmentInspectResult } from "../protocol/core-api.gen";
import { INLINE_MAX, addAll, attached, detail, pastedImage, sendable, size, willCopy } from "./attachments";

const info = (over: Partial<AttachmentInspectResult>): AttachmentInspectResult => ({
  path: "D:\\work\\repo\\notes.md",
  name: "notes.md",
  kind: "file",
  size_bytes: 2048,
  copied: true,
  ...over,
});

describe("an attachment's detail", () => {
  // verifies: FLOW-SEND-MESSAGE-R09
  it("is an image's size, a text file's line count, any other file's size, or that it is a folder", () => {
    expect(detail(info({ kind: "image", size_bytes: 86_016, media_type: "image/png" }))).toBe("84 KB");
    expect(detail(info({ lines: 212 }))).toBe("212 lines");
    expect(detail(info({ lines: 1 }))).toBe("1 line");
    expect(detail(info({ size_bytes: 3 * 1024 * 1024 }))).toBe("3 MB");
    expect(detail(info({ kind: "folder", size_bytes: undefined }))).toBe("Folder");
  });

  it("reads sizes in steps of 1024, with one decimal under ten", () => {
    expect(size(512)).toBe("512 bytes");
    expect(size(1)).toBe("1 byte");
    expect(size(1536)).toBe("1.5 KB");
    expect(size(1024)).toBe("1 KB");
    expect(size(5 * 1024 * 1024 * 1024)).toBe("5 GB");
    expect(size(1024 * 1024 - 1)).toBe("1 MB");
  });
});

describe("attachments that will be copied into the workspace", () => {
  // verifies: FLOW-SEND-MESSAGE-R17
  it("will be copied as Core said for the workspace, never judged from the path's text", () => {
    // Core resolved it: a path inside the folder that leads outside it, through a link or junction, is copied.
    expect(willCopy(attached(info({ path: "D:\\work\\repo\\vendor\\key.txt", copied: true }), "w-1"), "w-1")).toBe(true);
    expect(willCopy(attached(info({ path: "C:\\elsewhere\\a.md", copied: false }), "w-1"), "w-1")).toBe(false);
    // Judged for another workspace than the composer's now: taken to be copied, an image aside.
    expect(willCopy(attached(info({ copied: false }), "w-1"), "w-2")).toBe(true);
    expect(willCopy(attached(info({ copied: false }), "w-1"), undefined)).toBe(true);
    expect(willCopy(attached(info({ kind: "image", copied: false }), "w-1"), "w-2")).toBe(false);
    // For a workspace not yet made, as a new session's isolated one, Core says copied; an image never is.
    expect(willCopy(attached(info({ copied: true })), undefined)).toBe(true);
    expect(willCopy(attached(info({ kind: "image", media_type: "image/png", copied: false })), undefined)).toBe(false);
  });
});

describe("adding attachments", () => {
  // verifies: FLOW-SEND-MESSAGE-R12
  it("attaches the same file or folder once, however its path is written, keeping the order added", () => {
    const a = attached(info({ path: "D:\\work\\repo\\a.md", name: "a.md" }));
    const b = attached(info({ path: "D:\\work\\repo\\b.md", name: "b.md" }));
    const again = attached(info({ path: "d:/work/repo/A.md", name: "A.md" }));
    const list = addAll(addAll([], [a, b]), [again, b]);
    expect(list.map((x) => x.name)).toEqual(["a.md", "b.md"]);
  });

  // verifies: FLOW-SEND-MESSAGE-R13
  it("sends each by its kind and the path the user picked, and nothing else", () => {
    const image = attached(info({ path: "C:\\pics\\a.png", name: "a.png", kind: "image" }));
    const folder = attached(info({ path: "D:\\work\\repo\\src", name: "src", kind: "folder" }));
    expect(sendable([image, folder])).toEqual([
      { kind: "image", path: "C:\\pics\\a.png" },
      { kind: "folder", path: "D:\\work\\repo\\src" },
    ]);
  });
});

describe("a pasted image", () => {
  // verifies: FLOW-SEND-MESSAGE-R08
  // verifies: FLOW-SEND-MESSAGE-R10
  it("goes inline, and one too large to send or that can't be read is marked with why, unread", async () => {
    const png = await pastedImage(new Blob([new Uint8Array([1, 2, 3])], { type: "image/png" }));
    expect(png).toMatchObject({ name: "Pasted image", kind: "image", detail: "3 bytes", inline: { media_type: "image/png", data: btoa("\x01\x02\x03") } });
    expect(png.problem).toBeUndefined();
    // Too large: its size is known without reading it.
    let read = false;
    const huge = { size: INLINE_MAX + 1, type: "image/png", arrayBuffer: async () => ((read = true), new ArrayBuffer(0)) } as unknown as Blob;
    const big = await pastedImage(huge);
    expect(big).toMatchObject({ detail: "10 MB", problem: "This image is larger than 10 MB, the most Ersa sends in one message." });
    expect(big.inline).toBeUndefined();
    expect(read).toBe(false);
    const gone = { size: 10, type: "image/png", arrayBuffer: async () => Promise.reject(new Error("gone")) } as unknown as Blob;
    expect(await pastedImage(gone)).toMatchObject({ problem: "Ersa couldn't read this image." });
  });

  // verifies: FLOW-SEND-MESSAGE-R08
  it("keeps the pasted image's own kind, and two pastes are two attachments", async () => {
    const jpeg = await pastedImage(new Blob([new Uint8Array([255, 216, 255])], { type: "image/jpeg" }));
    const again = await pastedImage(new Blob([new Uint8Array([255, 216, 255])], { type: "image/jpeg" }));
    expect(sendable([jpeg])).toEqual([{ kind: "image", media_type: "image/jpeg", data: btoa("\xff\xd8\xff") }]);
    expect(addAll([jpeg], [again])).toHaveLength(2);
  });
});
