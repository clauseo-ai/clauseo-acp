// The words of the tool line's popover: each tool's state chip, Open Settings, and the Chrome download the browser
// row offers, in the fewest words (DESIGN.md §Icons). As in words-settings.ts the English is the key, the exact
// string on screen, and a key with slots fills them in either language. The popover loads after the first paint, so
// these words do too; the tools' names and reasons are words-tools-line.ts's. Its Hindi is in
// words-tools.hi.ts.
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-tools.hi.ts";

/** Every string of the tool line's popover, in English. */
export type ToolsKey = keyof typeof HI;

/** A popover string in the language, its slots filled. */
export const tw = wordsOf("tools");

/** The popover's words in the page's language, bound: const w = useToolsWords(); w.t("Download Chrome"). */
export function useToolsWords() {
  const { lang } = useDisplay();
  return { lang, t: (key: ToolsKey, slots?: Slots) => tw(lang, key, slots) };
}
