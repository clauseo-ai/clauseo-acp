import { DEFAULT_DISPLAY, TEXT_SIZES, type Display } from "@clauseo/components/lib/display.ts";
import { guessPlatform } from "../platform.ts";
import { PLATFORMS } from "./options.ts";

/** The first look from the page's query: theme=night, lang=hi, lite=1, motion=reduced, platform=windows|linux|mac,
 * size=18. Without a platform, the computer's own. */
export function displayFromQuery(search: string): Display {
  const q = new URLSearchParams(search);
  const size = Number(q.get("size"));
  return {
    theme: q.get("theme") === "night" ? "night" : "day",
    lang: q.get("lang") === "hi" ? "hi" : "en",
    render: q.get("lite") === "1" ? "lite" : "full",
    motion: q.get("motion") === "reduced" ? "reduced" : "full",
    platform: PLATFORMS.find((p) => p.value === q.get("platform"))?.value ?? guessPlatform(),
    textSize: TEXT_SIZES.find((s) => s === size) ?? DEFAULT_DISPLAY.textSize,
  };
}
