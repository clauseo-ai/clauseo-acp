// The words of the window's frame: the title bar on Windows, the plan meter's states, the matter list's states and
// the settings. As in words.ts the English is the key, the exact string on screen, and a key with slots fills them in
// either language; {time} takes clock(), so Hindi puts the part of the day first. The Hindi is a specialist's draft,
// waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script, which
// <Mixed> sets in lang="en" runs.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-frame.hi.ts";

/** Every string of the frame, in English. */
export type FrameKey = keyof typeof HI;

/** A frame string in the language, its slots filled. */
export const frame = wordsOf("frame");
