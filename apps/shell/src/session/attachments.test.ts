import { describe, expect, it } from "vitest";
import type { AttachmentInspectResult } from "../protocol/core-api.gen";
import { addAll, attached, detail, inside, outside, sendable, size } from "./attachments";

const info = (over: Partial<AttachmentInspectResult>): AttachmentInspectResult => ({
  path: "D:\\work\\repo\\notes.md",
  name: "notes.md",
  kind: "file",
  size_bytes: 2048,
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

describe("attachments from outside the workspace", () => {
  // verifies: FLOW-SEND-MESSAGE-R14
  it("are those the session's workspace does not hold, compared as Windows compares paths", () => {
    expect(outside(attached(info({})), "D:\\work\\repo")).toBe(false);
    expect(outside(attached(info({ path: "d:/WORK/repo/src" })), "D:\\work\\repo\\")).toBe(false);
    expect(outside(attached(info({ path: "D:\\work\\repository\\a.md" })), "D:\\work\\repo")).toBe(true);
    expect(outside(attached(info({ path: "C:\\Users\\me\\a.md" })), "D:\\work\\repo")).toBe(true);
    // A workspace not yet made, as a new session's, holds nothing yet.
    expect(outside(attached(info({})), undefined)).toBe(true);
    expect(inside("D:\\work\\repo", "D:\\work\\repo")).toBe(true);
    // A link is judged by where it points.
    expect(outside(attached(info({ target: "C:\\secrets\\a.md" })), "D:\\work\\repo")).toBe(true);
    expect(outside(attached(info({ path: "C:\\links\\a.md", target: "D:\\work\\repo\\a.md" })), "D:\\work\\repo")).toBe(false);
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
