// The usage limit's words that come with the open thread's chunk, never the first screen (U12): the stop at
// the limit in the thread, and the composer's warning, whose note is in the thread's chunk too. As in words.ts the English is the key, the exact string on screen, and a key with slots fills them
// in either language; {time} takes clock(), so Hindi puts the part of the day first. The Hindi is a specialist's draft,
// waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script. Nothing
// here says I or मैं, git, token, session or terminal, or pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-limits-thread.hi.ts";

/** Every string of the limit the thread brings, in English. */
export type LimitThreadKey = keyof typeof HI;

/** A string of the limit the thread brings, in the language, its slots filled. */
export const limThread = wordsOf("limits-thread");
