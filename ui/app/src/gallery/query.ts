import { DEFAULT_DISPLAY, TEXT_SIZES, type Display } from "@clauseo/components/lib/display.ts";

/** The first look from the page's query: theme=night, lang=hi, lite=1, motion=reduced, platform=windows, size=18. */
export function displayFromQuery(search: string): Display {
  const q = new URLSearchParams(search);
  const size = Number(q.get("size"));
  return {
    theme: q.get("theme") === "night" ? "night" : "day",
    lang: q.get("lang") === "hi" ? "hi" : "en",
    render: q.get("lite") === "1" ? "lite" : "full",
    motion: q.get("motion") === "reduced" ? "reduced" : "full",
    platform: q.get("platform") === "windows" ? "windows" : "mac",
    textSize: TEXT_SIZES.find((s) => s === size) ?? DEFAULT_DISPLAY.textSize,
  };
}
