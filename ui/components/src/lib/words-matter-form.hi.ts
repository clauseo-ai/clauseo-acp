// The Hindi of words-matter-form.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-matter-form.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  "A new matter": "नया मामला",
  "Name, as it stands on the cause list": "नाम, जैसा वाद सूची में दर्ज है",
  "Client, if any": "मुवक्किल, यदि कोई हो",
  "Next date, if fixed": "अगली तारीख, यदि तय हो",
  "30 October 2026": "30 अक्टूबर 2026",
  "Folder:": "फ़ोल्डर:",
  Change: "बदलें",
  // what Change changes, for a screen reader, where no folder line says it
  "The matter's folder": "मामले का फ़ोल्डर",
  "Clauseo makes this folder. Add papers to it any time.":
    "Clauseo यह फ़ोल्डर बनाता है। इसमें कभी भी दस्तावेज़ जोड़ें।",
  "Create the matter": "मामला बनाएँ",
  Back: "वापस",
  Cancel: "रद्द करें",
} as const;

/** Every string, English and Hindi, for the tests that hold the voice. */
export const matterFormWords: Hindi<keyof typeof HI> = HI;
