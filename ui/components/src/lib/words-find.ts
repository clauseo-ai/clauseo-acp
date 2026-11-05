// The words of Find's results (U16a): what a search inside the matters found, said on the rail under the field. Loaded
// with the results (room/find-results.tsx), never on first paint; the field's own word, "Find", stays in
// words-matters.ts. As in words.ts the English is the key. Every line here is new and its Hindi is still to come (null).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-find.hi.ts";

export type FindKey = keyof typeof HI;

/** Find's results' words in `lang`, their slots filled. */
export const fw = wordsOf("find");
