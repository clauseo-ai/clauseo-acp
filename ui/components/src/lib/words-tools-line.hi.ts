// The Hindi of words-tools-line.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-tools-line.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // each tool's name
  "Web search": "वेब खोज",
  Browser: "ब्राउज़र",
  // the tool line's name and tooltip, one sentence per tool: "Web search: on. Browser: off, needs Chrome."
  "{tool}: {state}.": "{tool}: {state}।",
  on: "चालू",
  // {reason} is one of the reasons below, its first letter in lower case
  "off, {reason}": "बंद, {reason}",
  // why a tool is off, a few words; In Settings sits beside the Off chip, so it says only where
  "In Settings": "सेटिंग्स में",
  "Managed by firm": "फ़र्म द्वारा प्रबंधित",
  "Can't search on this login": "इस लॉगिन पर खोज नहीं हो सकती",
  "Not available yet": "अभी उपलब्ध नहीं",
  "Too many browsers open": "बहुत सारे ब्राउज़र खुले हैं",
  "Needs Chrome": "Chrome चाहिए",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsLineWords: Hindi<keyof typeof HI> = HI;
