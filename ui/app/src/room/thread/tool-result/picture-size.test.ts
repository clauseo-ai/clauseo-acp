// The picture-size guard (U17): each format's header read at an allowed size; a side or an area past the caps, a
// truncated header, a broken JPEG segment chain and a declared type that isn't the header's all refused; a JPEG's frame
// found after its APP segments. Fixtures are tiny synthetic headers built here, never real pictures.
import { expect, test } from "vitest";
import { drawable, MAX_AREA, MAX_SIDE, pictureSize } from "./picture-size.ts";
import { readOutput } from "./parse.ts";

const b64 = (bytes: number[]) => btoa(bytes.map((b) => String.fromCharCode(b)).join(""));
const be16 = (n: number) => [(n >> 8) & 0xff, n & 0xff];
const be32 = (n: number) => [(n >>> 24) & 0xff, (n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
const le16 = (n: number) => [n & 0xff, (n >> 8) & 0xff];
const le24 = (n: number) => [n & 0xff, (n >> 8) & 0xff, (n >> 16) & 0xff];
const ascii = (s: string) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));

const png = (w: number, h: number) =>
  b64([
    0x89,
    0x50,
    0x4e,
    0x47,
    0x0d,
    0x0a,
    0x1a,
    0x0a,
    ...be32(13),
    ...ascii("IHDR"),
    ...be32(w),
    ...be32(h),
    8,
    6,
    0,
    0,
    0,
  ]);
const gif = (w: number, h: number) => b64([...ascii("GIF89a"), ...le16(w), ...le16(h), 0, 0, 0]);
const riff = (chunk: number[]) => [...ascii("RIFF"), ...le24(100), 0, ...ascii("WEBP"), ...chunk];
const vp8 = (w: number, h: number) =>
  b64(riff([...ascii("VP8 "), ...le24(50), 0, 0, 0, 0, 0x9d, 0x01, 0x2a, ...le16(w), ...le16(h)]));
const vp8l = (w: number, h: number) => {
  const bits = (w - 1) | ((h - 1) << 14);
  return b64(
    riff([
      ...ascii("VP8L"),
      ...le24(50),
      0,
      0x2f,
      bits & 0xff,
      (bits >> 8) & 0xff,
      (bits >> 16) & 0xff,
      (bits >>> 24) & 0xff,
    ]),
  );
};
const vp8x = (w: number, h: number) =>
  b64(riff([...ascii("VP8X"), ...le24(10), 0, 0, 0, 0, 0, ...le24(w - 1), ...le24(h - 1)]));
/** A JPEG: SOI, the segments given, then SOF0 of w × h. */
const sof = (w: number, h: number, marker = 0xc0) => [
  0xff,
  marker,
  ...be16(17),
  8,
  ...be16(h),
  ...be16(w),
  3,
];
const segment = (marker: number, body: number[]) => [
  0xff,
  marker,
  ...be16(body.length + 2),
  ...body,
];
const jpeg = (w: number, h: number, before: number[][] = []) =>
  b64([0xff, 0xd8, ...before.flat(), ...sof(w, h)]);

test("each format's header reads at an allowed size, and is drawn", () => {
  const cases: [string, string, number, number][] = [
    ["image/png", png(1280, 800), 1280, 800],
    ["image/jpeg", jpeg(1280, 800), 1280, 800],
    ["image/gif", gif(640, 480), 640, 480],
    ["image/webp", vp8(1280, 800), 1280, 800],
    ["image/webp", vp8l(1280, 8000), 1280, 8000],
    ["image/webp", vp8x(1280, 6000), 1280, 6000],
  ];
  for (const [mime, data, width, height] of cases) {
    expect(pictureSize(data)).toEqual({ format: mime, width, height });
    expect(drawable(mime, data)).toBe(true);
  }
});

test("a JPEG's frame is found after its APP segments, skipped by length; DHT, JPG and DAC are not frames", () => {
  const app0 = segment(0xe0, [...ascii("JFIF"), 0, 1, 2, 0, 0, 1, 0, 1, 0, 0]);
  const app1 = segment(
    0xe1,
    Array.from({ length: 3_000 }, (_, i) => i & 0xff),
  );
  const dht = segment(0xc4, [0, ...Array.from({ length: 16 }, () => 0)]);
  const dac = segment(0xcc, [0, 0]);
  const data = jpeg(4000, 3000, [app0, app1, dht, dac]);
  expect(pictureSize(data)).toEqual({ format: "image/jpeg", width: 4000, height: 3000 });
  expect(drawable("image/jpeg", data)).toBe(true);
  // a progressive frame (SOF2) reads the same
  expect(pictureSize(b64([0xff, 0xd8, ...app0, ...sof(10, 20, 0xc2)]))).toMatchObject({
    width: 10,
    height: 20,
  });
});

test("a picture past the side cap or the area cap is refused", () => {
  expect(drawable("image/png", png(MAX_SIDE, 100))).toBe(true);
  expect(drawable("image/png", png(MAX_SIDE + 1, 100))).toBe(false);
  expect(drawable("image/jpeg", jpeg(100, MAX_SIDE + 1))).toBe(false);
  expect(drawable("image/gif", gif(65_535, 1))).toBe(false);
  // 8,000 × 5,000 is the area cap exactly; one more row is past it
  expect(8_000 * 5_000).toBe(MAX_AREA);
  expect(drawable("image/png", png(8_000, 5_000))).toBe(true);
  expect(drawable("image/png", png(8_000, 5_001))).toBe(false);
  expect(drawable("image/webp", vp8x(10_000, 10_000))).toBe(false);
  // a PNG that says 2^32 - 1 wide
  expect(drawable("image/png", png(0xffffffff, 1))).toBe(false);
  // no pixels at all
  expect(drawable("image/png", png(0, 10))).toBe(false);
});

test("a header that can't be read is refused: truncated, a broken JPEG segment chain, or no frame within the bytes read", () => {
  expect(drawable("image/png", png(10, 10).slice(0, 20))).toBe(false);
  expect(drawable("image/gif", b64(ascii("GIF89a")))).toBe(false);
  expect(drawable("image/webp", vp8(10, 10).slice(0, 24))).toBe(false);
  expect(drawable("image/jpeg", jpeg(10, 10).slice(0, 8))).toBe(false);
  // a segment that doesn't start with 0xFF
  expect(drawable("image/jpeg", b64([0xff, 0xd8, 0x00, 0xe0, 0, 4, 0, 0, ...sof(10, 10)]))).toBe(
    false,
  );
  // a segment length under 2
  expect(drawable("image/jpeg", b64([0xff, 0xd8, 0xff, 0xe0, 0, 1, ...sof(10, 10)]))).toBe(false);
  // a scan before any frame
  expect(drawable("image/jpeg", b64([0xff, 0xd8, ...segment(0xda, [0, 0]), ...sof(10, 10)]))).toBe(
    false,
  );
  // the frame past the 64 KB read: APP segments of 60 KB each
  const big = segment(
    0xe1,
    Array.from({ length: 60_000 }, () => 0),
  );
  expect(drawable("image/jpeg", jpeg(10, 10, [big, big]))).toBe(false);
  // not base64 at all, and no known header
  expect(drawable("image/png", "####")).toBe(false);
  expect(drawable("image/png", b64(ascii("hello, world")))).toBe(false);
});

test("a declared type that isn't the header's is refused", () => {
  expect(drawable("image/jpeg", png(10, 10))).toBe(false);
  expect(drawable("image/png", jpeg(10, 10))).toBe(false);
  expect(drawable("image/webp", gif(10, 10))).toBe(false);
  expect(drawable("image/gif", vp8(10, 10))).toBe(false);
});

test("in a result, a refused picture is counted as not shown, never drawn", () => {
  const read = readOutput(
    JSON.stringify({
      content: [
        { type: "text", text: "Took a screenshot." },
        { type: "image", data: png(100_000, 100_000), mimeType: "image/png" },
        { type: "image", data: png(10, 10), mimeType: "image/jpeg" },
        { type: "image", data: png(1280, 800), mimeType: "image/png" },
      ],
    }),
  );
  expect(read).toMatchObject({ text: "Took a screenshot.", pictures: 2 });
  expect(read!.images).toEqual([{ mime: "image/png", data: png(1280, 800) }]);
});
