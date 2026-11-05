// The words of the usage limit the first screen shows: the toolbar's plan and its line, what a matter, the toolbar or
// the composer says once the plan's limit is reached, until the reset, and the reset words the stop and the plan's card
// share. The stop's own words and the composer's warning are in words-limits-thread.ts, loaded with the thread; the
// card's in words-plan-card.ts, loaded with the card. As in words.ts the English is the key, the exact string on
// screen, and a key with slots fills them in either language; {time} takes clock(), so Hindi puts the part of the day
// first. The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western
// digits, English names in Latin script. Nothing here says I or मैं, git, token, session or terminal, or pencil, ink,
// step, पेंसिल, स्याही, चरण or कदम.
import { clock } from "./words.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-limits.hi.ts";

/** Every string of the usage limit, in English. */
export type LimitKey = keyof typeof HI;

/** A string of the usage limit in the language, its slots filled. */
export const lim = wordsOf("limits");

/** The usage limit's words in the page's language, bound: const w = useLimitWords(); w.lim("Resets {time}", { time }). */
export function useLimitWords() {
  const { lang } = useDisplay();
  return {
    lang,
    lim: (key: LimitKey, slots?: Slots) => lim(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
