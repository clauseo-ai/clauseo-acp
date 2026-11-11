// The updater's words (R23, decision record 0037): the room's notice at the rail's foot (an update ready, waiting for
// the answer, installing, and once after an update) and Settings' Updates section (the version, the last check, the
// mode, Check now, and who manages updates where Clauseo doesn't). As in words.ts the English is the key.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-update.hi.ts";

export type UpdateKey = keyof typeof HI;

/** The updater's words in `lang`, their slots filled. */
export const uw = wordsOf("update");
