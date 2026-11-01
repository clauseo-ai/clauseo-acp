// The words of the usage limit: what a matter or the toolbar says once the plan's limit is reached, until the reset.
// As in words.ts the English is the key, the exact string on screen, and a key with slots fills them in either
// language; {time} takes clock(), so Hindi puts the part of the day first. The Hindi is a specialist's draft, waiting
// for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script. Nothing here
// says I or मैं, git, token, session or terminal, or pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

const HI = {
  // the limit reached, until the reset: today's clock time, or another day's with its day first; with no reset given,
  // the toolbar's no-reset form
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

/** The usage limit's words in the page's language: const w = useLimitWords(); w.lim("Limit reached. Reset not given."). */
export function useLimitWords() {
  const { lang } = useDisplay();
  return {
    lang,
    lim: (key: LimitKey, slots?: Slots) => lim(lang, key, slots),
  };
}
