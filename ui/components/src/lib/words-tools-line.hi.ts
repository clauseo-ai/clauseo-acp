// The Hindi of words-tools-line.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-tools-line.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // hi: later: every string here ships in English, its Hindi null until the Hindi pass
  // each tool's name
  "Web search": null,
  Browser: null,
  // the tool line's name and tooltip, one sentence per tool: "Web search: on. Browser: off, needs Chrome."
  "{tool}: {state}.": null,
  on: null,
  // {reason} is one of the reasons below, its first letter in lower case
  "off, {reason}": null,
  // why a tool is off, a few words; In Settings sits beside the Off chip, so it says only where
  "In Settings": null,
  "Managed by firm": null,
  "Can't search on this login": null,
  "Not available yet": null,
  "Too many browsers open": null,
  "Needs Chrome": null,
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsLineWords: Hindi<keyof typeof HI> = HI;
