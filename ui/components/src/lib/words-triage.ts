// The words of snooze and Done on the rail (U16b) drawn with the rail itself: a row's two buttons and its menus, the
// line left in a row's place, the folded Snoozed shelf and the folded Done fold. The words of what opens later (the
// snooze menu's times, the open shelf and the open fold) are words-triage-late.ts's, loaded with them. As in words.ts
// the English is the key. Every line here is new and its Hindi is still to come (null).
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-triage.hi.ts";

export type TriageKey = keyof typeof HI;

/** Snooze and Done's words in `lang`, their slots filled. */
export const tw = wordsOf("triage");
