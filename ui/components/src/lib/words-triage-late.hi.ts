// The Hindi of words-triage-late.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-triage-late.ts imports this file's type alone.

export const HI = {
  // RO-07: the menu's heading, and the row of a matter's menu that opens it (RO-15)
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
  // RO-13: a conversation's two buttons and its menu's items, and a done conversation's button
  Snooze: null,
  "Mark as done": null,
  "Mark as not done": null,
  // RO-15: a matter's menu, with RO-07's "Snooze until" on its submenu's row
  "Mark all done": null,
  // RO-17: behind the open fold's info icon
  "Conversations you've read, with nothing waiting, move here each morning.": null,
} as const;
