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
  /** The window's width to render at, in CSS px, before the frame scales it to its cell. Defaults to DESKTOP. */
  width?: number;
  /** The window's height, in CSS px. Defaults to DESKTOP. */
  height?: number;
};

/** The window a state renders in unless its entry says otherwise: a desktop window between the room's 1000 and
 * 1200 px breakpoints, at the room's least height (--window-min-h). */
export const DESKTOP = { width: 1100, height: 600 } as const;

/** An entry's current text in both languages. */
export const currentOf = (e: Entry): Current => ({
  id: e.id,
  en: e.words.text("en"),
  hi: e.words.text("hi"),
});
