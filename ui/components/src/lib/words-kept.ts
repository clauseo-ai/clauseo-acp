// The words of a matter's kept permissions: the rules "Always for this matter" kept for Claude's
// permission requests, listed from the rail's matter menu (room/kept-permissions.tsx), each with
// a Remove. As in words.ts the English is the key, the exact string on screen.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-kept.hi.ts";

/** Every string of the kept permissions list, in English. */
export type KeptKey = keyof typeof HI;

/** A kept permissions string in the language, its slots filled. */
export const kp = wordsOf("kept");
