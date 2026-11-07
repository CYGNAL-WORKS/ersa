import { describe, expect, it } from "vitest";
import { imageSize } from "./imageSize";

const base64 = (bytes: number[]) => btoa(String.fromCharCode(...bytes));
const be16 = (n: number) => [(n >> 8) & 0xff, n & 0xff];
const le16 = (n: number) => [n & 0xff, (n >> 8) & 0xff];
const be32 = (n: number) => [...be16(n >>> 16), ...be16(n & 0xffff)];
const text = (s: string) => [...s].map((c) => c.charCodeAt(0));

describe("an image's size from its header", () => {
  // verifies: FLOW-SEND-MESSAGE-R11
  it("reads PNG, GIF, WebP and JPEG, and nothing from data that states none", () => {
    const png = [0x89, ...text("PNG"), 0x0d, 0x0a, 0x1a, 0x0a, ...be32(13), ...text("IHDR"), ...be32(640), ...be32(480), 8, 6, 0, 0, 0];
    expect(imageSize(base64(png))).toEqual({ width: 640, height: 480 });

    const gif = [...text("GIF89a"), ...le16(32), ...le16(16), 0, 0, 0];
    expect(imageSize(base64(gif))).toEqual({ width: 32, height: 16 });

    const riff = (chunk: string, body: number[]) => [...text("RIFF"), 0, 0, 0, 0, ...text("WEBP"), ...text(chunk), 0, 0, 0, 0, ...body];
    // Lossy: a frame tag, the start code, then 14-bit width and height.
    const vp8 = riff("VP8 ", [0, 0, 0, 0x9d, 0x01, 0x2a, ...le16(300), ...le16(200)]);
    expect(imageSize(base64(vp8))).toEqual({ width: 300, height: 200 });
    // Lossless: the signature byte, then width - 1 and height - 1 in 14 bits each.
    const w = 99, h = 49;
    const vp8l = riff("VP8L", [0x2f, w & 0xff, ((w >> 8) & 0x3f) | ((h & 0x03) << 6), (h >> 2) & 0xff, (h >> 10) & 0x0f, 0, 0, 0, 0, 0, 0, 0, 0]);
    expect(imageSize(base64(vp8l))).toEqual({ width: 100, height: 50 });
    // Extended: flags, then width - 1 and height - 1 in 24 bits each.
    const vp8x = riff("VP8X", [0, 0, 0, 0, 1023 & 0xff, 1023 >> 8, 0, 767 & 0xff, 767 >> 8, 0]);
    expect(imageSize(base64(vp8x))).toEqual({ width: 1024, height: 768 });

    // A JPEG with an APP0 segment before its frame header.
    const jpeg = [0xff, 0xd8, 0xff, 0xe0, ...be16(16), ...text("JFIF"), 0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xff, 0xc0, ...be16(17), 8, ...be16(600), ...be16(800), 3, 0, 0, 0];
    expect(imageSize(base64(jpeg))).toEqual({ width: 800, height: 600 });

    expect(imageSize(base64(text("not an image at all")))).toBeUndefined();
    expect(imageSize("%%% not base64")).toBeUndefined();
  });
});
