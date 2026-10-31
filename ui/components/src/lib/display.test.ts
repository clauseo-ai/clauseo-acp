// @vitest-environment happy-dom
// Hindi's faces, 726 KB of Anek Devanagari and the Latin set inside Hindi, load only for a display in Hindi: at start,
// or on the first switch to it, and once.
import { afterEach, beforeEach, expect, test, vi } from "vitest";

let load: ReturnType<typeof vi.fn<(font: string, text?: string) => Promise<FontFace[]>>>;

beforeEach(() => {
  vi.resetModules();
  load = vi.fn(() => Promise.resolve([]));
  Object.defineProperty(document, "fonts", { configurable: true, value: { load } });
});

afterEach(() => {
  Reflect.deleteProperty(document, "fonts");
});

const families = () => new Set(load.mock.calls.map(([font]) => font.replace(/^\d+ \d+px /, "")));

test("an English start loads none of Hindi's faces, and the first switch to Hindi loads them once", async () => {
  const { DEFAULT_DISPLAY, startDisplay, showDisplay } = await import("./display.ts");
  const en = { ...DEFAULT_DISPLAY, motion: "reduced" as const };
  startDisplay(en);
  expect(load).not.toHaveBeenCalled();
  showDisplay({ ...en, theme: "night" });
  expect(load).not.toHaveBeenCalled();
  showDisplay({ ...en, lang: "hi" });
  expect(families()).toEqual(new Set(["'Anek Devanagari'", "'Anek Latin Hi'"]));
  const calls = load.mock.calls.length;
  showDisplay(en);
  showDisplay({ ...en, lang: "hi" });
  expect(load).toHaveBeenCalledTimes(calls);
});

test("a Hindi start loads Hindi's faces at 400 and 600, and the probe's size", async () => {
  const { DEFAULT_DISPLAY, startDisplay } = await import("./display.ts");
  startDisplay({ ...DEFAULT_DISPLAY, lang: "hi" });
  const fonts = load.mock.calls.map(([font]) => font);
  for (const f of [
    "400 16px 'Anek Devanagari'",
    "600 16px 'Anek Devanagari'",
    "400 100px 'Anek Devanagari'",
  ])
    expect(fonts).toContain(f);
  expect(fonts).toContain("600 16px 'Anek Latin Hi'");
});
