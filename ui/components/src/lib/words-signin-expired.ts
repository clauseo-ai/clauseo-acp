// The words of the expired sign-in in the room's thread (UD2, limits/signin): the card that holds the question while
// the plan signs in again, and what it says for each way back. In the thread's chunk with the card, never on first
// paint. As in words.ts the English is the key. English only for now (hi: later); the design's own lines keep their
// words, with the plan's name as a slot.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-signin-expired.hi.ts";

export type SigninExpiredKey = keyof typeof HI;

/** The expired sign-in's words in `lang`, their slots filled. */
export const sx = wordsOf("signin-expired");
