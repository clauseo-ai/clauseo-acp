// The words of snooze and Done that open later (U16b): the snooze menu's times, the open Snoozed shelf and the open
// Done fold, loaded with them (room/snooze-late.tsx), never on first paint. The rest are words-triage.ts's. As in
// words.ts the English is the key. Every line here is new and its Hindi is still to come (null).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-triage-late.hi.ts";

export type TriageLateKey = keyof typeof HI;

/** The later words of snooze and Done in `lang`, their slots filled. */
export const twl = wordsOf("triage-late");
