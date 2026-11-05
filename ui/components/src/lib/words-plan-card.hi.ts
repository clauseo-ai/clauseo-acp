// The Hindi of words-plan-card.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-plan-card.ts imports this file's type alone.
import type { Hindi } from "./words-core.ts";

export const HI = {
  // the windows, by the label the host gives; a label not here shows as the host gives it (the last three are U12's)
  "5-hour limit": "5 घंटे की सीमा",
  "Weekly limit": "साप्ताहिक सीमा",
  // a per-model window, {model} as the host names it, never written here (#236)
  "{model} weekly limit": "{model} की साप्ताहिक सीमा",
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

/** Every key, for the banned-words screen in the tests. */
export const planCardWords: Hindi<keyof typeof HI> = HI;
