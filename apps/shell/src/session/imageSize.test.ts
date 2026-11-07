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

    // A segment before the frame holding the bytes FF C0 is passed over whole, not read as the frame.
    const decoy = [0xff, 0xd8, 0xff, 0xe2, ...be16(10), 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 2, 0xff, 0xc0, ...be16(17), 8, ...be16(600), ...be16(800), 3, 0, 0, 0];
    expect(imageSize(base64(decoy))).toEqual({ width: 800, height: 600 });

    expect(imageSize(base64(text("not an image at all")))).toBeUndefined();
    expect(imageSize("%%% not base64")).toBeUndefined();
  });
  // verifies: FLOW-SEND-MESSAGE-R11
  it("swaps a JPEG's width and height when its EXIF orientation turns it a quarter", () => {
    // EXIF in Motorola order: a directory of one entry, Orientation (0x0112), SHORT, count 1, the value.
    const exif = (orientation: number) => {
      const body = [...text("Exif"), 0, 0, ...text("MM"), 0, 42, ...be32(8), ...be16(1), ...be16(0x0112), ...be16(3), ...be32(1), ...be16(orientation), 0, 0, ...be32(0)];
      return [0xff, 0xe1, ...be16(body.length + 2), ...body];
    };
    const photo = (orientation: number) => [0xff, 0xd8, ...exif(orientation), 0xff, 0xc0, ...be16(17), 8, ...be16(3024), ...be16(4032), 3, 0, 0, 0];
    // Every quarter turn swaps, the transposed and transverse ones too; the half turn and upright don't.
    for (const turned of [5, 6, 7, 8]) expect(imageSize(base64(photo(turned)))).toEqual({ width: 3024, height: 4032 });
    for (const upright of [1, 2, 3, 4]) expect(imageSize(base64(photo(upright)))).toEqual({ width: 4032, height: 3024 });
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("reads the orientation in Intel order too, and keeps it past a later metadata segment", () => {
    const le32 = (n: number) => [n & 0xff, (n >> 8) & 0xff, (n >> 16) & 0xff, (n >>> 24) & 0xff];
    // EXIF in Intel order, as many Android phones and cameras write it.
    const intel = [...text("Exif"), 0, 0, ...text("II"), 42, 0, ...le32(8), ...le16(1), ...le16(0x0112), ...le16(3), ...le32(1), ...le16(6), 0, 0, ...le32(0)];
    // An XMP segment after the EXIF one, as phones and editors often add.
    const xmp = text("http://ns.adobe.com/xap/1.0/\0<x:xmpmeta/>");
    const segment = (body: number[]) => [0xff, 0xe1, ...be16(body.length + 2), ...body];
    const photo = [0xff, 0xd8, ...segment(intel), ...segment(xmp), 0xff, 0xc0, ...be16(17), 8, ...be16(3024), ...be16(4032), 3, 0, 0, 0];
    expect(imageSize(base64(photo))).toEqual({ width: 3024, height: 4032 });
  });

  // verifies: FLOW-SEND-MESSAGE-R11
  it("reads no orientation past the end of its own segment", () => {
    // A directory that claims two entries where the segment's length holds one: the second, an Orientation of 6,
    // follows in the file but is not this segment's, so the photo is not turned.
    const entry = (tag: number, value: number) => [...be16(tag), ...be16(3), ...be32(1), ...be16(value), 0, 0];
    const held = [...text("Exif"), 0, 0, ...text("MM"), 0, 42, ...be32(8), ...be16(2), ...entry(0x0128, 2)];
    const beyond = entry(0x0112, 6);
    const photo = [0xff, 0xd8, 0xff, 0xe1, ...be16(held.length + 2), ...held, ...beyond, 0xff, 0xc0, ...be16(17), 8, ...be16(3024), ...be16(4032), 3, 0, 0, 0];
    expect(imageSize(base64(photo))).toEqual({ width: 4032, height: 3024 });
  });});
