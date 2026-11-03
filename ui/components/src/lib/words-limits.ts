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

const HI = {
  // the thread: when the limit resets, today at a time, or on a day
  "Resets {time}": "{time} बजे रीसेट",
  "Resets {day} {time}": "{day} {time} बजे रीसेट",
  // when the limit reset, once it has: earlier today, or on a day before
  "The limit reset at {time}.": "सीमा {time} बजे रीसेट हुई।",
  "The limit reset on {day} at {time}.": "सीमा {day} {time} बजे रीसेट हुई।",
  // the limit row's own line under its headline, when the reset isn't given
  "Reset not given.": "रीसेट का समय नहीं बताया गया।",
  // a workspace out of credits or at its spend cap, where "Claude plan limit reached" isn't true
  "Usage limit reached": "उपयोग की सीमा पूरी हो गई",
  "Try again": "फिर से प्रयास करें",
  // a matter or the toolbar: the limit reached, until the reset: today's clock time, or another day's with its day
  // first; with no reset given, the toolbar's no-reset form (the thread says it too)
  "Limit reached until {time}": "{time} बजे तक सीमा पूरी",
  "Limit reached until {day} {time}": "{day} {time} बजे तक सीमा पूरी",
  "Limit reached. Reset not given.": "सीमा पूरी। रीसेट का समय नहीं बताया गया।",
  // the composer's status line while the conversation is paused at the limit and the stop's time is known
  "Stopped at {time}, at the plan's limit": "{time} बजे, प्लान की सीमा पर रुका",
  "Stopped on {day} at {time}, at the plan's limit": "{day} को {time} बजे, प्लान की सीमा पर रुका",

  /* ---------- U12: the plan in the toolbar ---------- */
  // the toolbar's plan billed by use, and its line; the provider other than Anthropic gets no line (U12's own words)
  "{name}, pay as you go": "{name}, उपयोग के अनुसार भुगतान",
  "Billed to your Anthropic account": "आपके Anthropic खाते में शुल्क लगता है",
  // the toolbar's line by the share used: under half, under three quarters, close; {time} is the reset, its day first
  // when it isn't today. Without a reset given, the line's first words alone (U12's own words)
  "Plenty left until the {time} reset": "{time} बजे के रीसेट तक काफ़ी बाकी है",
  "Under half left. Resets at {time}.": "आधे से कम बाकी। {time} बजे रीसेट।",
  "Close to the limit. Resets at {time}.": "सीमा के करीब। {time} बजे रीसेट होगा।",
  "Plenty left": "काफ़ी बाकी है",
  "Under half left.": "आधे से कम बाकी।",
  "Close to the limit.": "सीमा के करीब।",
  "Usage not shown right now": "अभी उपयोग नहीं दिख रहा",
  "{n} percent of the plan left": "प्लान का {n} प्रतिशत बाकी है",
  "Show the plan's limits": "प्लान की सीमाएँ दिखाएँ",
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
