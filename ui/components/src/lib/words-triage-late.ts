// The words of snooze and Done that open later (U16b): the snooze menu's times, the open Snoozed shelf and the open
// Done fold, loaded with them (room/snooze-late.tsx), never on first paint. The rest are words-triage.ts's. As in
// words.ts the English is the key. Every line here is new and its Hindi is still to come (null).
import { wordsOf } from "./words-core.ts";

const HI = {
  // RO-07: the menu's heading
  "Snooze until": null,
  // RO-08: the menu's times, and the picker
  "Tomorrow {time}": null,
  "Monday {time}": null,
  "Choose a day and time": null,
  // RO-09: with "{weekday} {d} {month}, {time}" beneath, 9:00 am the day before the matter's next date
  "Before the next date": null,
  // RO-05: a snoozed row's line in the shelf; {when} as in "Next back {when}"
  "Until {when}": null,
  // RO-06: a matter snoozed whole, in the shelf
  "Whole matter": null,
  // RO-12: a shelf row's button
  "Wake now": null,
  // RO-04: behind the open shelf's info icon
  "Back sooner if Clauseo needs you or an answer finishes.": null,
  // RO-13: a done conversation's button
  "Mark as not done": null,
  // RO-17: behind the open fold's info icon
  "Read conversations with nothing waiting move here each morning.": null,
} as const;

export type TriageLateKey = keyof typeof HI;

/** The later words of snooze and Done in `lang`, their slots filled. */
export const twl = wordsOf<TriageLateKey>(HI);
