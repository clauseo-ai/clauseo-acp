// The words of the first read (UD2): Clauseo's own read of a matter added during the first run, its conversation's
// title, how its question shows, and the line that says how far it has got. Loaded with the first read's chunk
// (room/first-read/) and the thread's, never on first paint. As in words.ts the English is the key. English only for
// now (hi: later).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-first-read.hi.ts";

export type FirstReadKey = keyof typeof HI;

/** The first read's words in `lang`, their slots filled. */
export const frd = wordsOf("first-read");
