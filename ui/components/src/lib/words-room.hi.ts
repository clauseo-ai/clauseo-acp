// The Hindi of words-room.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-room.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the "N more" fold at the wall's foot, once it's open: it folds the rest of the citations away again
  "Show fewer": "कम दिखाएँ",
  // the toolbar's name for assistive technology: search, Versions and Undo
  "Tools for this matter": "इस मामले के टूल",
  // the room's line, one line in every room, saying what the Safe workspace does and promising nothing.
  // Inside a matter it names this matter's folder; where no matter is open yet, each matter's.
  "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.":
    "सुरक्षित कार्यक्षेत्र। Clauseo इस मामले के फ़ोल्डर के भीतर काम करता है और उससे बाहर जाने से पहले पूछता है।",
} as const;

/** The room's own strings, for the tests' banned-words screen. */
export const roomHindi: Hindi<keyof typeof HI> = HI;
