// The words of the new matter form, in English and Hindi. The form is a part of its own, opened by
// the first launch and by the rail's New matter, so its words live apart from either screen.
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-matter-form.hi.ts";

export type MatterFormKey = keyof typeof HI;
export const mf = wordsOf("matter-form");

/** The form's words in the page's language: const w = useMatterFormWords(); w("Create matter"). */
export function useMatterFormWords() {
  const { lang } = useDisplay();
  return (key: MatterFormKey, slots?: Slots) => mf(lang, key, slots);
}
