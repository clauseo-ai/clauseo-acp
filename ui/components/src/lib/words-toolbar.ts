// The few words of Versions and protection the room's first load shows: the toolbar's Versions, its history card's
// name, and the Not protected mark beside a matter's name, in the toolbar and on the rail. Each string's one home; the
// rest of Versions' words (words-versions.ts, which spreads these in so vt still reads them) load with Versions.
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";

export const TOOLBAR_WORDS = {
  Versions: "संस्करण",
  "Not protected": "संरक्षित नहीं",
  "Not protected.": "संरक्षित नहीं।",
  "Clauseo can reach beyond this matter's folder without asking.":
    "Clauseo बिना पूछे इस मामले के फ़ोल्डर से बाहर पहुँच सकता है।",
} as const;
export type ToolbarKey = keyof typeof TOOLBAR_WORDS;

/** A toolbar string in the language, its slots filled. */
export const tbt = wordsOf<ToolbarKey>(TOOLBAR_WORDS);

/** The toolbar's words of the page's language, bound. */
export function useToolbarWords() {
  const { lang } = useDisplay();
  return { lang, v: (key: ToolbarKey, slots?: Slots) => tbt(lang, key, slots) };
}
