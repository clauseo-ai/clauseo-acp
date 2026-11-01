// One registered string: the shape registry.tsx's entries take.
import type { Lang } from "@clauseo/components/lib/display.ts";
import type { ReactNode } from "react";
import type { Current } from "./status.ts";
import type { WordsRef } from "./words.ts";

export type Entry = {
  /** Short, stable and typeable: the brief, a dash, a number: "U1-01". */
  id: string;
  words: WordsRef;
  /** The screen that shows it, as page/id: "room/toolbar". */
  screen: string;
  /** The state that shows the string, in the real parts, for one language. */
  render: (lang: Lang) => ReactNode;
};

/** An entry's current text in both languages. */
export const currentOf = (e: Entry): Current => ({
  id: e.id,
  en: e.words.text("en"),
  hi: e.words.text("hi"),
});
