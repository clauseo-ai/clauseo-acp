// What every words file shares: the slots a string takes and the lookup that fills them. Each words file holds only
// its own list: English is the key, Hindi the value, and wordsOf turns the list into its lookup. A new string may ship
// with null Hindi ("hi: later"): the Hindi interface shows its English until the Hindi comes.
// The Hindi lists live apart from the lookups, in each words file's .hi.ts beside it, and reach the page as one lazy
// chunk (words-hi.ts) only when the display is Hindi: an English page never loads them. Each lookup finds its list here,
// in the registry that chunk fills; display.ts loads it, and holds a Hindi display back until it is in.
import type { Lang } from "./display.ts";
import type { HINDI } from "./words-hi.ts";

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

/** Every words file's Hindi list, by its area: the words file's name less "words-" ("words" for words.ts). */
export type HindiLists = typeof HINDI;
export type HindiArea = keyof HindiLists;

const NONE: Hindi<string> = {};
let lists: Readonly<Record<HindiArea, Hindi<string>>> | null = null;

/** Fills the registry: words-hi.ts, when its chunk loads or a test or dev page imports it. */
export function registerHindi(all: HindiLists): void {
  lists = all;
}

/** Whether the Hindi lists are in. */
export const hindiLoaded = (): boolean => lists !== null;

let loading: Promise<void> | null = null;

/** Loads the Hindi chunk once and fills the registry; a load that fails is tried again by the next call. */
export function loadHindi(): Promise<void> {
  if (lists !== null) return Promise.resolve();
  loading ??= import("./words-hi.ts").then(
    (m) => {
      registerHindi(m.HINDI);
    },
    (error: unknown) => {
      loading = null;
      throw error;
    },
  );
  return loading;
}

/** Empties the registry, as a page that has not loaded the Hindi chunk has it. For tests only. */
export function forgetHindiForTests(): void {
  lists = null;
  loading = null;
}

/** A words file's lookup: an English key in, the string in the language out, its slots filled. A key with null Hindi
 * gives its English in Hindi too, as does every key while the Hindi chunk is not in (display.ts never shows a Hindi
 * page then). `hi` is the area's list, for the copy review to tell hi: later from Hindi: empty until the chunk is in. */
export function wordsOf<A extends HindiArea>(area: A) {
  type K = keyof HindiLists[A] & string;
  return wordsOver<K>(() => (lists?.[area] ?? NONE) as Hindi<K>);
}

/** The lookup over a list got when it is asked for: wordsOf's, over the registry's area. */
export function wordsOver<K extends string>(list: () => Hindi<K>) {
  const say = (lang: Lang, key: K, slots?: Slots): string =>
    fill(lang === "hi" ? (list()[key] ?? key) : key, slots);
  return Object.defineProperty(say, "hi", { get: list, enumerable: true }) as typeof say & {
    readonly hi: Hindi<K>;
  };
}

// File names as a line or a dialog shows them, shared by every words file that names a paper: Versions' undo and
// restore lines, the change note and the thread's notices. In the first load (display.ts imports this file), so a lazy
// chunk that names a file takes no chunk of its own for it.

/** A path or a name set apart from the words around it (FSI…PDI), so a right-to-left mark in it can't reorder them. */
export const isolate = (text: string): string => `\u2068${text}\u2069`;

/** A file's bare name: the last part of its path in the matter folder. */
export const baseName = (path: string): string => path.split(/[\\/]/).pop() ?? path;

/** A file's name as shown: its bare name, isolated. */
export const fileName = (path: string): string => isolate(baseName(path));

/** The joiner of a list, its own words: "{a} and {b}" in the list area (words-list.hi.ts). */
const joined = wordsOf("list");

/** "A", "A and B", "A, B and C", in the language: the list's names, joined as the sentence joins them. */
export function listOf(lang: Lang, items: readonly string[]): string {
  if (items.length <= 1) return items[0] ?? "";
  const head = items.slice(0, -1).join(", ");
  return joined(lang, "{a} and {b}", { a: head, b: items[items.length - 1] ?? "" });
}

/** Each file's name in a dialog or a line, by its path: the name alone, or, where two of the same list's files share a
 * name, the whole path; isolated either way. */
export function namesOf(paths: readonly string[]): (path: string) => string {
  const count = new Map<string, number>();
  for (const p of new Set(paths)) count.set(baseName(p), (count.get(baseName(p)) ?? 0) + 1);
  return (p) => ((count.get(baseName(p)) ?? 0) > 1 ? isolate(p) : fileName(p));
}
