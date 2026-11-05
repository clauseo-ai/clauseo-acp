// The plan's card's own words (U12), loaded with the card: the windows by the label the host gives, the share
// left, the read's time, and the card with no figures or billed by use. Its reset words, and Try again, are the stop's
// too (words-limits.ts). As in words.ts the English is the key, the exact string on screen, and a key with slots fills them
// in either language; {time} takes clock(), so Hindi puts the part of the day first. The Hindi is a specialist's draft,
// waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script. Nothing
// here says I or मैं, git, token, session or terminal, or pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-plan-card.hi.ts";

/** Every string of the plan's card, in English. */
export type PlanCardKey = keyof typeof HI;

/** A string of the plan's card in the language, its slots filled. */
export const pc = wordsOf("plan-card");
