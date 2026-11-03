// The plan's card's own words (U12), loaded with the card: the windows by the label the host gives, the share
// left, the read's time, and the card with no figures or billed by use. Its reset words, and Try again, are the stop's
// too (words-limits.ts). As in words.ts the English is the key, the exact string on screen, and a key with slots fills them
// in either language; {time} takes clock(), so Hindi puts the part of the day first. The Hindi is a specialist's draft,
// waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script. Nothing
// here says I or मैं, git, token, session or terminal, or pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { wordsOf } from "./words-core.ts";

const HI = {
  // the windows, by the label the host gives; a label not here shows as the host gives it (the last five are U12's)
  "5-hour limit": "5 घंटे की सीमा",
  "Weekly limit": "साप्ताहिक सीमा",
  "Opus weekly limit": "Opus की साप्ताहिक सीमा",
  "Sonnet weekly limit": "Sonnet की साप्ताहिक सीमा",
  "Weekly limit for connected apps": "जुड़े हुए ऐप्स की साप्ताहिक सीमा",
  "Monthly limit": "मासिक सीमा",
  "Usage limit": "उपयोग की सीमा",
  "{n}% left": "{n}% बाकी",
  "As of {time}": "{time} बजे के आँकड़े",
  // the card with no figures; the second line names the plan, Claude or ChatGPT (U12's form of the design's)
  "Usage isn't available right now.": "अभी उपयोग की जानकारी उपलब्ध नहीं है।",
  "Your {name} plan didn't send its limits this time. Asking still works as usual.":
    "आपके {name} प्लान ने इस बार अपनी सीमाएँ नहीं भेजीं। पूछना पहले की तरह चलता है।",
  // billed by use, to Anthropic
  "Pay as you go, billed to your Anthropic account":
    "उपयोग के अनुसार भुगतान, आपके Anthropic खाते में शुल्क लगता है",
} as const;

/** Every string of the plan's card, in English. */
export type PlanCardKey = keyof typeof HI;

/** A string of the plan's card in the language, its slots filled. */
export const pc = wordsOf<PlanCardKey>(HI);

/** Every key, for the banned-words screen in the tests. */
export const planCardWords: Readonly<Record<PlanCardKey, string>> = HI;
