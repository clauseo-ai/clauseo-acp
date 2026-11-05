// The words of the live thread: the working line, errors, the permission prompt, the change note, the composer and
// the empty states. As in words.ts the English is the key, the exact string on screen, and a key with slots fills
// them in either language.
// The Hindi is a specialist's draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits,
// English legal names in Latin script. Clauseo never says मैं, and nothing says पेंसिल, स्याही, चरण or कदम.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-thread.hi.ts";

/** Every new string of the live thread, in English. */
export type ThreadKey = keyof typeof HI;

/** A new string in the language, its slots filled. */
export const th = wordsOf("thread");

/** The thread's words of the page's language, bound, with the shared ones beside them:
 * const w = useThreadWords(); w.th("Allow once"), w.t("Undo"), w.ui("Undo anyway"). */
export function useThreadWords() {
  const { lang } = useDisplay();
  return {
    lang,
    th: (key: ThreadKey, slots?: Slots) => th(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
