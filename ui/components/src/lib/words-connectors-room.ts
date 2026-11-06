// The room's connector words (U15): the case-law hint under an answer and the out-of-credit note under the composer's
// field. As in words.ts the English is the key, the exact string on screen; English only for now (hi: later). The
// price fills its slot from money.ts. Only the thread's chunk imports this file, so the room's first load has none of
// these words.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-connectors-room.hi.ts";

/** Every string of the room's connector parts, in English. */
export type ConnectorsRoomKey = keyof typeof HI;

/** A room connector string in the language, its slots filled. */
export const cr = wordsOf("connectors-room");
