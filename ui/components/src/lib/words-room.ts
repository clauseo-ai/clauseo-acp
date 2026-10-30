// The room's own strings, beyond the shared ones in words.ts. The key is the exact English, with {slots} where it's
// built from parts. None of them uses मैं, पेंसिल, स्याही, चरण or
// कदम, or "pencil", "ink" or "step".
import type { Lang } from "./display.ts";
import { fill, type Slots } from "./words-core.ts";
import { t, type Key } from "./words.ts";

export const roomWords = {
  hi: {
    // the "N more" fold at the wall's foot, once it's open: it folds the rest of the citations away again
    "Show fewer": "कम दिखाएँ",
    // the toolbar's name for assistive technology: search, History and Undo
    "Tools for this matter": "इस मामले के टूल",
    // the room's line, one line in every room, saying what the Safe workspace does and promising nothing.
    // Inside a matter it names this matter's folder; where no matter is open yet, each matter's.
    "Safe workspace. Clauseo works inside this matter's folder and asks before going beyond it.":
      "सुरक्षित कार्यक्षेत्र। Clauseo इस मामले के फ़ोल्डर के भीतर काम करता है और उससे बाहर जाने से पहले पूछता है।",
  },
} as const;

export type RoomKey = keyof typeof roomWords.hi;

const isRoomKey = (key: string): key is RoomKey => key in roomWords.hi;

/** A string in the language: the room's own first, then the language's. */
export function rt(lang: Lang, key: RoomKey | Key, slots?: Slots): string {
  if (isRoomKey(key)) {
    const s = lang === "hi" ? roomWords.hi[key] : key;
    return fill(s, slots);
  }
  return t(lang, key, slots);
}
