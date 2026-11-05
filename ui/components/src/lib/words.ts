// The interface's words. English is the key, the exact string on screen, and Hindi is the value beside it. A key
// with slots, such as "{n} more", fills them in either language. Parts hold no strings of their own: each takes its
// words from here or from the words file of its own area (words-ui.ts, words-frame.ts). The Hindi is a specialist's
// draft, waiting for a Hindi-speaking lawyer's review: the formal आप, the danda, Western digits, and English names in
// Latin script.

import { useDisplay, type Lang } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words.hi.ts";

/** Every interface string this file has in both languages. */
export type Key = keyof typeof HI;

/** A string in the language, its slots filled. */
export const t = wordsOf("words");

/** "4:31 pm" in the language: Hindi puts the part of the day first, "शाम 4:31", "दोपहर 3:58". */
export function clock(lang: Lang, time: string): string {
  if (lang === "en") return time;
  return time.replace(/(\d{1,2}:\d{2})\s*(am|pm)/i, (_m: string, tm: string, ap: string) => {
    if (ap.toLowerCase() === "am") return t(lang, "{time} am", { time: tm });
    const h = Number.parseInt(tm, 10) % 12;
    const key: Key = h < 4 ? "{time} pm, afternoon" : h < 7 ? "{time} pm" : "{time} pm, night";
    return t(lang, key, { time: tm });
  });
}

/** The words of the page's language, bound: const w = useWords(); w.t("{n} more", { n: 3 }). */
export function useWords() {
  const { lang } = useDisplay();
  return {
    lang,
    t: (key: Key, slots?: Slots) => t(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
