// The words of snooze and Done on the rail (U16b) drawn with the rail itself: a row's two buttons and its menus, the
// line left in a row's place, the folded Snoozed shelf and the folded Done fold. The words of what opens later (the
// snooze menu's times, the open shelf and the open fold) are words-triage-late.ts's, loaded with them. As in words.ts
// the English is the key. Every line here is new and its Hindi is still to come (null).
import { wordsOf } from "./words-core.ts";

const HI = {
  // RO-13: a conversation's two buttons, and its menu
  Snooze: null,
  "Mark as done": null,
  // RO-15: a matter's menu
  "Snooze until…": null,
  "Mark all done": null,
  // RO-02: the shelf's heading, with its count
  Snoozed: null,
  // RO-03: under the folded shelf; {when} is "6:00 pm", "tomorrow 9:00 am" or "Monday 9:00 am"
  "Next back {when}": null,
  // RO-16: the fold's heading, with its count
  Done: null,
  // RO-10 (with Undo) in a row's place after snoozing, RO-11 a snoozed conversation's mark, named
  "Snoozed until {when}": null,
  // RO-14 (with Undo) in a row's place after marking it done
  "Marked as done": null,
  // RO-33: a time tomorrow, inside the lines above and RO-05
  "tomorrow {time}": null,
} as const;

export type TriageKey = keyof typeof HI;

/** Snooze and Done's words in `lang`, their slots filled. */
export const tw = wordsOf<TriageKey>(HI);
