// The Hindi of words-tools-line.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-tools-line.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // each tool's name
  "Web search": "वेब खोज",
  Browser: "ब्राउज़र",
  // each tool's state, after its name on the status line and in the popover's chip
  On: "चालू",
  Off: "बंद",
  // the tool line's name and tooltip, one sentence per tool: "Web search: on. Browser: off, install Chrome."
  "{tool}: {state}.": "{tool}: {state}।",
  on: "चालू",
  // {reason} is one of the reasons below, its first letter in lower case
  "off, {reason}": "बंद, {reason}",
  // why a tool is off, a few words; In Settings sits beside the Off chip, so it says only where
  "In Settings": "सेटिंग्स में",
  "Managed by firm": "फ़र्म द्वारा प्रबंधित",
  "Can't search on this login": "इस लॉगिन पर खोज नहीं हो सकती",
  // no browser on this computer: the lawyer's step, by the computer's OS (Chrome on a Mac, Chrome or Edge on Windows)
  // (provisional Hindi, app track final)
  "Install Chrome": "Chrome इंस्टॉल करें",
  "Install Chrome or Edge": "Chrome या Edge इंस्टॉल करें",
  // the firm's Chrome or Edge policy turns off remote debugging, so no browser starts
  // (provisional Hindi, app track final)
  "Blocked by a browser policy": "ब्राउज़र की एक नीति ने रोका है",
  // Clauseo couldn't read that browser policy, so no browser starts until it can
  // (provisional Hindi, app track final)
  "Couldn't check browser settings": "ब्राउज़र सेटिंग्स की जाँच नहीं हो सकी",
  // Windows: the only Chrome is a copy installed for this user alone, which Clauseo's browser can't start
  // (UC-554; provisional Hindi, app track final)
  "Can't use your Chrome": "आपका Chrome इस्तेमाल नहीं हो सकता",
  // Clauseo couldn't start the browser's safe connection for this conversation, so no browser starts in it
  // (provisional Hindi, app track final)
  "Couldn't start a safe connection": "सुरक्षित कनेक्शन शुरू नहीं हो सका",
  // Clauseo's unsafe-site list didn't load as the conversation started, so no browser starts in it
  // (provisional Hindi, app track final)
  "Couldn't load the unsafe-site list": "असुरक्षित वेबसाइटों की सूची लोड नहीं हो सकी",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const toolsLineWords: Hindi<keyof typeof HI> = HI;
