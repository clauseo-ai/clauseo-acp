// What every words file shares: the slots a string takes and the lookup that fills them. Each words file holds only
// its own list: English is the key, Hindi the value, and wordsOf turns the list into its lookup. A new string may ship
// with null Hindi ("hi: later"): the Hindi interface shows its English until the Hindi comes.
import type { Lang } from "./display.ts";

/** The values for a string's {slots}, by name. */
export type Slots = Record<string, string | number>;

/** A string with its {slots} filled; a slot with no value stays as written. */
export const fill = (s: string, slots?: Slots) =>
  slots === undefined
    ? s
    : s.replace(/\{(\w+)\}/g, (m: string, k: string) => (k in slots ? String(slots[k]) : m));

/** A words file's list: each English key's Hindi, or null while its Hindi is still to come (hi: later). Every key
 * needs an entry; null stands for the Hindi, never for the key. */
export type Hindi<K extends string> = Readonly<Record<K, string | null>>;

/** A words file's lookup: an English key in, the string in the language out, its slots filled. A key with null Hindi
 * gives its English in Hindi too. `hi` is the list, for the copy review to tell hi: later from Hindi. */
export function wordsOf<K extends string>(hi: Hindi<K>) {
  const say = (lang: Lang, key: K, slots?: Slots): string =>
    fill(lang === "hi" ? (hi[key] ?? key) : key, slots);
  return Object.assign(say, { hi });
}
