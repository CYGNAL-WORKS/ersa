import { describe, expect, it } from "vitest";
import type { AttachmentInspectResult } from "../protocol/core-api.gen";
import { addAll, attached, detail, sendable, size, willCopy } from "./attachments";

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
  // verifies: FLOW-SEND-MESSAGE-R14
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
