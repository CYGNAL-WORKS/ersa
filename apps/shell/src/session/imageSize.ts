// An image's size as its own header states it (DESIGN.md, Layout: an attached image's box is sized before it is
// drawn). PNG, GIF, WebP and JPEG all give their width and height in their first bytes, which a message's base64
// already carries, so the transcript lays the box out at once and nothing moves when the image draws.

export interface ImageSize {
  width: number;
  height: number;
}

/** How much of the base64 is read: a JPEG's frame header can follow its metadata, which is seldom this long. */
const HEAD_CHARS = 64 * 1024;

/** The width and height \`data\` (base64) states in its header, or undefined when it states none this reads. */
export function imageSize(data: string): ImageSize | undefined {
  let bytes: Uint8Array;
  try {
    const head = data.slice(0, HEAD_CHARS - (HEAD_CHARS % 4));
    bytes = Uint8Array.from(atob(head.slice(0, head.length - (head.length % 4))), (c) => c.charCodeAt(0));
  } catch {
    return undefined;
  }
  const size = png(bytes) ?? gif(bytes) ?? webp(bytes) ?? jpeg(bytes);
  return size && size.width > 0 && size.height > 0 ? size : undefined;
}

const u16le = (b: Uint8Array, at: number) => (b[at] ?? 0) | ((b[at + 1] ?? 0) << 8);
const u32le = (b: Uint8Array, at: number) => ((u16le(b, at + 2) << 16) >>> 0) + u16le(b, at);
const u16be = (b: Uint8Array, at: number) => ((b[at] ?? 0) << 8) | (b[at + 1] ?? 0);
const u24le = (b: Uint8Array, at: number) => u16le(b, at) | ((b[at + 2] ?? 0) << 16);
const u32be = (b: Uint8Array, at: number) => ((u16be(b, at) << 16) >>> 0) + u16be(b, at + 2);
const ascii = (b: Uint8Array, at: number, text: string) => [...text].every((c, i) => b[at + i] === c.charCodeAt(0));

/** PNG: the IHDR chunk, first after the signature. */
function png(b: Uint8Array): ImageSize | undefined {
  if (b.length < 24 || b[0] !== 0x89 || !ascii(b, 1, "PNG") || !ascii(b, 12, "IHDR")) return undefined;
  return { width: u32be(b, 16), height: u32be(b, 20) };
}

/** GIF: the logical screen size after the signature. */
function gif(b: Uint8Array): ImageSize | undefined {
  if (b.length < 10 || !ascii(b, 0, "GIF8")) return undefined;
  return { width: u16le(b, 6), height: u16le(b, 8) };
}

/** WebP: a lossy (VP8), lossless (VP8L) or extended (VP8X) first chunk. */
function webp(b: Uint8Array): ImageSize | undefined {
  if (b.length < 30 || !ascii(b, 0, "RIFF") || !ascii(b, 8, "WEBP")) return undefined;
  if (ascii(b, 12, "VP8 ")) return { width: u16le(b, 26) & 0x3fff, height: u16le(b, 28) & 0x3fff };
  if (ascii(b, 12, "VP8L")) {
    const [b0 = 0, b1 = 0, b2 = 0, b3 = 0] = b.subarray(21, 25);
    return { width: 1 + (((b1 & 0x3f) << 8) | b0), height: 1 + (((b3 & 0x0f) << 10) | (b2 << 2) | ((b1 & 0xc0) >> 6)) };
  }
  if (ascii(b, 12, "VP8X")) return { width: 1 + u24le(b, 24), height: 1 + u24le(b, 27) };
  return undefined;
}

/** JPEG: the first start-of-frame segment, past whatever segments come before it, turned by its EXIF orientation. */
function jpeg(b: Uint8Array): ImageSize | undefined {
  if (b[0] !== 0xff || b[1] !== 0xd8) return undefined;
  let at = 2;
  let quarterTurn = false;
  while (at + 9 < b.length) {
    if (b[at] !== 0xff) {
      at += 1;
      continue;
    }
    const marker = b[at + 1] ?? 0;
    // Start-of-frame markers, leaving out DHT (C4), JPG (C8) and DAC (CC), which share the range.
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      const width = u16be(b, at + 7);
      const height = u16be(b, at + 5);
      // A phone photo is often stored sideways, with an orientation that turns it upright a quarter.
      return quarterTurn ? { width: height, height: width } : { width, height };
    }
    if (marker === 0xe1) quarterTurn = exifQuarterTurn(b, at + 4, u16be(b, at + 2) - 2) || quarterTurn;
    // Markers that stand alone, with no length.
    if (marker === 0xff || marker === 0x01 || (marker >= 0xd0 && marker <= 0xd8)) {
      at += marker === 0xff ? 1 : 2;
      continue;
    }
    at += 2 + u16be(b, at + 2);
  }
  return undefined;
}

/**
 * Whether the EXIF data in an APP1 segment (`length` bytes from `start`) orients the image a quarter turn: an
 * Orientation tag (0x0112) of 5 to 8 in the first image directory.
 */
function exifQuarterTurn(b: Uint8Array, start: number, length: number): boolean {
  if (!ascii(b, start, "Exif") || b[start + 4] !== 0 || b[start + 5] !== 0) return false;
  const tiff = start + 6;
  const end = start + length;
  const little = ascii(b, tiff, "II");
  if (!little && !ascii(b, tiff, "MM")) return false;
  const u16 = (at: number) => (little ? u16le(b, at) : u16be(b, at));
  const u32 = (at: number) => (little ? u32le(b, at) : u32be(b, at));
  const directory = tiff + u32(tiff + 4);
  const entries = u16(directory);
  for (let i = 0; i < entries; i += 1) {
    const entry = directory + 2 + i * 12;
    if (entry + 12 > end) return false;
    if (u16(entry) === 0x0112) {
      const orientation = u16(entry + 8);
      return orientation >= 5 && orientation <= 8;
    }
  }
  return false;
}
