// A picture's pixel size, read from its own header before it is drawn (U17): any MCP server's result can carry one,
// and a small file can decode to a huge bitmap. Only a bounded prefix of the base64 is decoded: the first 64 KB of
// bytes, enough for a JPEG's segments before its frame. Pure: parse.ts asks drawable() for each image block.

/** The formats read, as the mimeType each one is declared with. */
export type Format = "image/png" | "image/jpeg" | "image/gif" | "image/webp";
export type Size = { format: Format; width: number; height: number };

/** The widest or tallest picture drawn, in pixels. */
export const MAX_SIDE = 16_384;
/** The most pixels a picture drawn may have. */
export const MAX_AREA = 40_000_000;
/** The most bytes read for a header: a JPEG's frame must start within them. */
export const MAX_HEADER_BYTES = 65_536;

/** The first `MAX_HEADER_BYTES` bytes of base64 `data`, null when that prefix isn't base64. */
function prefixOf(data: string): Uint8Array | null {
  // whole groups of 4 characters decode on their own; the rest of the data stays undecoded
  const chars = Math.min(data.length, Math.floor(MAX_HEADER_BYTES / 3) * 4);
  const text = chars === data.length ? data : data.slice(0, chars - (chars % 4));
  try {
    const binary = atob(text);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
    return bytes;
  } catch {
    return null;
  }
}

const be16 = (b: Uint8Array, at: number) => (b[at]! << 8) | b[at + 1]!;
const le16 = (b: Uint8Array, at: number) => b[at]! | (b[at + 1]! << 8);
const be32 = (b: Uint8Array, at: number) => be16(b, at) * 65_536 + be16(b, at + 2);
const le24 = (b: Uint8Array, at: number) => b[at]! | (b[at + 1]! << 8) | (b[at + 2]! << 16);
const ascii = (b: Uint8Array, at: number, text: string) => {
  if (b.length < at + text.length) return false;
  for (let i = 0; i < text.length; i += 1) if (b[at + i] !== text.charCodeAt(i)) return false;
  return true;
};

const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/** PNG: the signature, then IHDR's width and height. */
function png(b: Uint8Array): Size | null {
  if (b.length < 24 || !PNG_SIGNATURE.every((v, i) => b[i] === v) || !ascii(b, 12, "IHDR"))
    return null;
  return { format: "image/png", width: be32(b, 16), height: be32(b, 20) };
}

/** GIF: the logical screen's width and height. */
function gif(b: Uint8Array): Size | null {
  if (b.length < 10 || !(ascii(b, 0, "GIF87a") || ascii(b, 0, "GIF89a"))) return null;
  return { format: "image/gif", width: le16(b, 6), height: le16(b, 8) };
}

/** WebP: RIFF…WEBP, then the size its first chunk gives (VP8, VP8L or VP8X). */
function webp(b: Uint8Array): Size | null {
  if (!ascii(b, 0, "RIFF") || !ascii(b, 8, "WEBP")) return null;
  if (ascii(b, 12, "VP8 ")) {
    if (b.length < 30 || b[23] !== 0x9d || b[24] !== 0x01 || b[25] !== 0x2a) return null;
    return { format: "image/webp", width: le16(b, 26) & 0x3fff, height: le16(b, 28) & 0x3fff };
  }
  if (ascii(b, 12, "VP8L")) {
    if (b.length < 25 || b[20] !== 0x2f) return null;
    const [b0, b1, b2, b3] = [b[21]!, b[22]!, b[23]!, b[24]!];
    return {
      format: "image/webp",
      width: 1 + (((b1 & 0x3f) << 8) | b0),
      height: 1 + (((b3 & 0x0f) << 10) | (b2 << 2) | ((b1 & 0xc0) >> 6)),
    };
  }
  if (ascii(b, 12, "VP8X")) {
    if (b.length < 30) return null;
    return { format: "image/webp", width: 1 + le24(b, 24), height: 1 + le24(b, 27) };
  }
  return null;
}

/** JPEG: SOI, then segments skipped by their length until the first SOF0–SOF15 (not DHT C4, JPG C8 or DAC CC). A
 * segment chain that breaks, an end or a scan before the frame, or a frame past the bytes read, reads as nothing. */
function jpeg(b: Uint8Array): Size | null {
  if (b.length < 4 || b[0] !== 0xff || b[1] !== 0xd8) return null;
  let at = 2;
  while (at + 1 < b.length) {
    if (b[at] !== 0xff) return null;
    const marker = b[at + 1]!;
    // fill bytes before a marker
    if (marker === 0xff) {
      at += 1;
      continue;
    }
    // markers that stand alone: TEM and RST0–RST7
    if (marker === 0x01 || (marker >= 0xd0 && marker <= 0xd7)) {
      at += 2;
      continue;
    }
    // SOI again, EOI or a scan before any frame
    if (marker === 0xd8 || marker === 0xd9 || marker === 0xda) return null;
    if (at + 3 >= b.length) return null;
    const length = be16(b, at + 2);
    if (length < 2) return null;
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      if (length < 7 || at + 8 >= b.length) return null;
      return { format: "image/jpeg", height: be16(b, at + 5), width: be16(b, at + 7) };
    }
    at += 2 + length;
  }
  return null;
}

/** The picture's format and pixel size, read from the first bytes of its base64; null when no header reads. */
export function pictureSize(data: string): Size | null {
  const b = prefixOf(data);
  if (b === null) return null;
  return png(b) ?? jpeg(b) ?? gif(b) ?? webp(b);
}

/** Whether a picture is drawn: its header reads, is the format its mimeType declares, and its size is within
 * MAX_SIDE on each side and MAX_AREA in all. */
export function drawable(mime: string, data: string): boolean {
  const size = pictureSize(data);
  if (size === null || size.format !== mime) return false;
  const { width, height } = size;
  return (
    width >= 1 &&
    height >= 1 &&
    width <= MAX_SIDE &&
    height <= MAX_SIDE &&
    width * height <= MAX_AREA
  );
}
