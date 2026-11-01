// The words of the usage limit: the thread's stop at the limit and its reset, and what a matter or the toolbar says
// once the plan's limit is reached, until the reset. As in words.ts the English is the key, the exact string on
// screen, and a key with slots fills them in either language; {time} takes clock(), so Hindi puts the part of the day
// first. The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western
// digits, English names in Latin script. Nothing here says I or मैं, git, token, session or terminal, or pencil, ink,
// step, पेंसिल, स्याही, चरण or कदम.
import { clock } from "./words.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // the thread: when the limit resets, today at a time, or on a day
  "Resets {time}": "{time} बजे रीसेट",
  "Resets {day} {time}": "{day} {time} बजे रीसेट",
  // a workspace out of credits or at its spend cap, where "Claude plan limit reached" isn't true
  "Usage limit reached": "उपयोग की सीमा पूरी हो गई",
  "Try again": "फिर से प्रयास करें",
  // a matter or the toolbar: the limit reached, until the reset: today's clock time, or another day's with its day
  // first; with no reset given, the toolbar's no-reset form (the thread says it too)
  "Limit reached until {time}": "{time} बजे तक सीमा पूरी",
  "Limit reached until {day} {time}": "{day} {time} बजे तक सीमा पूरी",
  "Limit reached. Reset not given.": "सीमा पूरी। रीसेट का समय नहीं बताया गया।",
} as const;

/** Every string of the usage limit, in English. */
export type LimitKey = keyof typeof HI;

/** A string of the usage limit in the language, its slots filled. */
export const lim = wordsOf<LimitKey>(HI);

/** Every key, for the banned-words screen in the tests. */
export const limitWords: Readonly<Record<LimitKey, string>> = HI;

/** The usage limit's words in the page's language, bound: const w = useLimitWords(); w.lim("Resets {time}", { time }). */
export function useLimitWords() {
  const { lang } = useDisplay();
  return {
    lang,
    lim: (key: LimitKey, slots?: Slots) => lim(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
