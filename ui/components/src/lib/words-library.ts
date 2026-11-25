// Skills and connectors, the rail's place (24 Nov round, library L1: UC-1050 to UC-1089, UC-1215 to
// UC-1238; L2: UC-1301 to UC-1336). As in words.ts the English is the key, the exact string on
// screen; L1's Hindi comes later (null), L2's is provisional. Clauseo Case Law's agreed words are
// the home card's (words-connectors-room.ts, UC-1090 to UC-1095), reused.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-library.hi.ts";

/** Every string of Skills and connectors, in English. */
export type LibraryKey = keyof typeof HI;

/** A Skills and connectors string in the language, its slots filled. */
export const lb = wordsOf("library");
