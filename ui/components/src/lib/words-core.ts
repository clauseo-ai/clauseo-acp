// What every words file shares: the slots a string takes and the lookup that fills them. Each words file holds only
// its own list: English is the key, Hindi the value, and wordsOf turns the list into its lookup.
import type { Lang } from "./display.ts";

/** The values for a string's {slots}, by name. */
export type Slots = Record<string, string | number>;

/** A string with its {slots} filled; a slot with no value stays as written. */
export const fill = (s: string, slots?: Slots) =>
  slots === undefined
    ? s
    : s.replace(/\{(\w+)\}/g, (m: string, k: string) => (k in slots ? String(slots[k]) : m));

/** A words file's lookup: an English key in, the string in the language out, its slots filled. */
export function wordsOf<K extends string>(hi: Readonly<Record<K, string>>) {
  return (lang: Lang, key: K, slots?: Slots): string => fill(lang === "hi" ? hi[key] : key, slots);
}
