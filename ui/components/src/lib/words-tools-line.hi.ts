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
  // the firm's Chrome or Edge policy turns off remote debugging, so no browser starts
  // (provisional Hindi, app track final)
  "Blocked by your firm": "आपकी फ़र्म ने रोका है",
  // Clauseo couldn't read that browser policy, so no browser starts until it can
  // (provisional Hindi, app track final)
  "Couldn't check browser settings": "ब्राउज़र सेटिंग्स की जाँच नहीं हो सकी",
  // the Chrome Clauseo downloaded is too old to use; the row offers the download
  // (provisional Hindi, app track final)
  "Needs a newer Chrome": "नया Chrome चाहिए",
  // Clauseo has no verified list of Chrome versions, so it can't check its Chrome; no download helps
  // (provisional Hindi, app track final)
  "Couldn't check Chrome's version": "Chrome के संस्करण की जाँच नहीं हो सकी",
  // Clauseo's newest list of Chrome versions is too old, so its Chrome is too, and it can't update it yet; no
  // download helps (provisional Hindi, app track final)
  "Can't update Chrome right now": "Chrome अभी अपडेट नहीं हो सकता",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsLineWords: Hindi<keyof typeof HI> = HI;
