// The words of the window's frame: the title bar on Windows, the plan meter's states, the matter list's states and
// the settings. As in words.ts the English is the key, the exact string on screen, and a key with slots fills them in
// either language; {time} takes clock(), so Hindi puts the part of the day first. The Hindi is a specialist's draft,
// waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script, which
// <Mixed> sets in lang="en" runs.
import { wordsOf } from "./words-core.ts";

const HI = {
  // the title bar on Windows: the caption buttons and their group; Restore and Close are words-ui.ts's
  Window: "विंडो",
  Minimise: "न्यूनतम करें",
  Maximise: "अधिकतम करें",
  // the matter list: what each matter is doing
  "Answering. Expected by {time}": "उत्तर तैयार हो रहा है। {time} बजे तक अपेक्षित",
  "A question for you": "आपसे एक प्रश्न",
  "Needs your permission": "आपकी अनुमति चाहिए",
  "Answer ready": "उत्तर तैयार है",
  "Stopped at {time}": "{time} बजे रोका गया",
  // paused at the plan's limit, until the reset; the usage limit
  "Paused until {time}": "{time} बजे तक रुका है",
  "Paused until {day} {time}": "{day} {time} बजे तक रुका है",
  "Paused at the plan's limit": "प्लान की सीमा पर रुका है",
  "Limit reached. Continue at {time}?": "सीमा पूरी। {time} बजे जारी\u00a0रखें?",
  "Limit reached. Continue on {day} at {time}?": "सीमा पूरी। {day} को {time} बजे जारी\u00a0रखें?",
  Pinned: "पिन किए गए",
  "No matters yet": "अभी कोई मामला नहीं",
  "Add a matter's folder. Clauseo reads its papers before your first question.":
    "मामले का फ़ोल्डर जोड़ें। आपके पहले प्रश्न से पहले Clauseo उसके दस्तावेज़ पढ़ लेता है।",
} as const;

/** Every string of the frame, in English. */
export type FrameKey = keyof typeof HI;

/** A frame string in the language, its slots filled. */
export const frame = wordsOf<FrameKey>(HI);
