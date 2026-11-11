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
  "Safe workspace. Clauseo works only in this matter's folder. It can't open your other files unless you allow it.":
    "सुरक्षित कार्यक्षेत्र। Clauseo सिर्फ़ इस मामले के फ़ोल्डर में काम करता है। आपकी अनुमति के बिना वह आपकी दूसरी फ़ाइलें नहीं खोल सकता।",
  // after D2d, when Claude works on its own inside the Safe workspace (safe-workspace-line.tsx)
  "Safe workspace. Clauseo changes files in this matter without asking. It can't open your other files unless you allow it.":
    "सुरक्षित कार्यक्षेत्र। Clauseo बिना पूछे इस मामले की फ़ाइलें बदलता है। आपकी अनुमति के बिना वह आपकी दूसरी फ़ाइलें नहीं खोल सकता।",
  // the one-time note on Windows while Smart App Control is still in evaluation (room/sac/). "Smart App Control"
  // stays in English, as Windows shows it.
  "If Windows ever blocks Clauseo, turn off Smart App Control.":
    "अगर Windows कभी Clauseo को रोके, तो Smart App Control बंद करें।",
  // the note's link: the install guide's Smart App Control section, through the link confirm
  "How to do this": "यह कैसे करें",
} as const;

/** The room's own strings, for the tests' banned-words screen. */
export const roomHindi: Hindi<keyof typeof HI> = HI;
