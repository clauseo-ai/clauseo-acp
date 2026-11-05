// The reader's words: the judgment reader, Indian Kanoon's results and its guard, the CCI and NCLAT index, citation
// checks in a draft, and Word's tracked changes. As in words.ts the English is the key, the exact string on screen,
// and a key with slots fills them in either language; {time} takes clock(), so Hindi puts the part of the day first.
// The Hindi uses the formal आप, the danda, Western digits, and English names in Latin script. "powered by IKanoon" is
// Indian Kanoon's own line and stays in English. Judgments' names, dates and words are content, shown as given.
import { clock, t as roomWords, type Key } from "./words.ts";
import { ui, type UiKey } from "./words-ui.ts";
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-reader.hi.ts";

/** Every string of the reader, in English. */
export type ReaderKey = keyof typeof HI;

/** A string of the reader in the language, its slots filled. */
export const rdt = wordsOf("reader");

/** The reader's words in the display's language, bound, with the room's and the interface's beside them. */
export function useReaderWords() {
  const { lang } = useDisplay();
  return {
    lang,
    r: (key: ReaderKey, slots?: Slots) => rdt(lang, key, slots),
    t: (key: Key, slots?: Slots) => roomWords(lang, key, slots),
    ui: (key: UiKey, slots?: Slots) => ui(lang, key, slots),
    clock: (time: string) => clock(lang, time),
  };
}
