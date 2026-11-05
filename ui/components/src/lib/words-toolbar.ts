// The few words of Versions and protection the room's first load shows: the toolbar's Versions, its history card's
// name, and the Not protected mark beside a matter's name, in the toolbar and on the rail. Each string's one home; the
// rest of Versions' words (words-versions.ts, which spreads these in so vt still reads them) load with Versions.
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-toolbar.hi.ts";

export type ToolbarKey = keyof typeof HI;

/** A toolbar string in the language, its slots filled. */
export const tbt = wordsOf("toolbar");

/** The toolbar's words of the page's language, bound. */
export function useToolbarWords() {
  const { lang } = useDisplay();
  return { lang, v: (key: ToolbarKey, slots?: Slots) => tbt(lang, key, slots) };
}
