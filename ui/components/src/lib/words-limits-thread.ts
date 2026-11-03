// The usage limit's words that come with the open thread's chunk, never the first screen (U12): the stop at
// the limit in the thread, and the composer's warning, whose note is in the thread's chunk too. As in words.ts the English is the key, the exact string on screen, and a key with slots fills them
// in either language; {time} takes clock(), so Hindi puts the part of the day first. The Hindi is a specialist's draft,
// waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script. Nothing
// here says I or मैं, git, token, session or terminal, or pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { wordsOf, type Hindi } from "./words-core.ts";

const HI = {
  // the stop: its title, with the time the answer stopped, or without one where the stop's time isn't known; {name} is
  // Claude, or ChatGPT for Codex
  "{name} plan limit reached at {time}": "{time} बजे {name} प्लान की सीमा पूरी हुई",
  "{name} plan limit reached on {day} at {time}": "{day} को {time} बजे {name} प्लान की सीमा पूरी हुई",
  "{name} plan limit reached": "{name} प्लान की सीमा पूरी हो गई",
  // the window's line under the title: today's reset, or another day's; never a span, which would need a clock
  "The 5-hour limit. It resets at {time}.": "5 घंटे की सीमा। यह {time} बजे रीसेट होगी।",
  "The 5-hour limit. It resets on {day} at {time}.":
    "5 घंटे की सीमा। यह {day} को {time} बजे रीसेट होगी।",
  "The weekly limit. It resets at {time}.": "साप्ताहिक सीमा। यह {time} बजे रीसेट होगी।",
  "The weekly limit. It resets on {day} at {time}.":
    "साप्ताहिक सीमा। यह {day} को {time} बजे रीसेट होगी।",
  // the mark where the answer stopped at the limit, today or on another day
  "Stopped here at {time}": "{time} बजे यहाँ रुका",
  "Stopped here on {day} at {time}": "{day} को {time} बजे यहाँ रुका",
  // the note under the field, in two sentences so the reset stands in its own <time> (the design's one sentence, cut);
  // another window than the 5-hour one leads with the toolbar's "Close to the limit." (words-limits.ts)
  "Close to your 5-hour limit.": "आपकी 5 घंटे की सीमा करीब है।",
  "Long tasks may pause until {time}.": "लंबे काम {time} बजे तक रुक सकते हैं।",
  "Long tasks may pause until {day} {time}.": "लंबे काम {day} {time} बजे तक रुक सकते हैं।",
} as const;

/** Every string of the limit the thread brings, in English. */
export type LimitThreadKey = keyof typeof HI;

/** A string of the limit the thread brings, in the language, its slots filled. */
export const limThread = wordsOf<LimitThreadKey>(HI);

/** Every key, for the banned-words screen in the tests. */
export const limitThreadWords: Hindi<LimitThreadKey> = HI;
