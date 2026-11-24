// Skills and connectors, the rail's place (24 Nov round, library L1: UC-1050 to UC-1089, UC-1215 to
// UC-1238). As in words.ts the English is the key, the exact string on screen; the Hindi comes
// later (null). Clauseo Case Law's agreed words are the home card's (words-connectors-room.ts,
// UC-1090 to UC-1095), reused.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-library.hi.ts";

/** Every string of Skills and connectors, in English. */
export type LibraryKey = keyof typeof HI;

/** A Skills and connectors string in the language, its slots filled. */
export const lb = wordsOf("library");
