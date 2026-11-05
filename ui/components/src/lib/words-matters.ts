// The words of living in matters: finding a matter on the rail, a folder that moved, the conversations inside a
// matter, and messages sent while Clauseo works. As in words.ts the English is the key, the exact string on screen,
// and a key with slots fills them in either language; {time} takes clock(), so Hindi puts the part of the day first.
// The Hindi uses the formal आप, the danda, Western digits and English names in Latin script. Nothing here says I or
// मैं, names a model, or says pencil, ink, step, पेंसिल, स्याही, चरण or कदम.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { frame, type FrameKey } from "./words-frame.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-matters.hi.ts";

/** Every string of living in matters, in English. */
export type MatterKey = keyof typeof HI;

/** A string of living in matters in the language, its slots filled. */
export const mt = wordsOf("matters");

/** The words of living in matters, bound to the page's language, with the shared, interface and frame words beside
 * them: const w = useMatterWords(); w.m("Send now"), w.t("Matters"), w.f("Pinned"). */
export function useMatterWords() {
  const { lang } = useDisplay();
  return {
    lang,
    m: (key: MatterKey, slots?: Slots) => mt(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    f: (key: FrameKey, slots?: Slots) => frame(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
