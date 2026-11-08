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
  // the one-time note on Windows while Smart App Control is still in evaluation (room/sac/). Windows Security's own
  // labels stay in English, as Windows shows them.
  "Only if Windows ever blocks Clauseo on this computer: open Windows Security, go to App & browser control, then Smart App Control settings, and turn it off. On an up-to-date Windows 11, you can turn it back on later. Until then, there's nothing to do.":
    "केवल तभी, जब Windows कभी इस कंप्यूटर पर Clauseo को रोके: Windows Security खोलें, App & browser control में जाएँ, फिर Smart App Control settings में इसे बंद करें। अप-टू-डेट Windows 11 पर आप इसे बाद में फिर से चालू कर सकते हैं। तब तक कुछ करने की ज़रूरत नहीं है।",
  // the note's link: the install guide's Smart App Control section, through the link confirm
  "How to do this": "यह कैसे करें",
} as const;

/** The room's own strings, for the tests' banned-words screen. */
export const roomHindi: Hindi<keyof typeof HI> = HI;
