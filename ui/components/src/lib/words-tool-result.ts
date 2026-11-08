// What came back from a tool call (U17): the press under an open group's row and the block it opens. As in words.ts
// the English is the key, the exact string on screen; English only for now (hi: later). Only the thread's chunk
// imports this file, so the room's first load has none of these words.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-tool-result.hi.ts";

/** Every string of a row's result, in English. */
export type ToolResultKey = keyof typeof HI;

/** A result string in the language, its slots filled. */
export const tr = wordsOf("tool-result");
