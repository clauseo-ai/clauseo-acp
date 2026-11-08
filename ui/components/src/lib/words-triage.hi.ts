// The Hindi of words-triage.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-triage.ts imports this file's type alone.

export const HI = {
  // RO-02: the shelf's heading, with its count
  Snoozed: "टाले गए",
  // RO-03: under the folded shelf; {when} is "6:00 pm", "tomorrow 9:00 am" or "Monday 9:00 am"
  "Next back {when}": "अगली वापसी {when}",
  // RO-16: the fold's heading, with its count
  Done: "पूरे हुए",
  // RO-10 (with Undo) in a row's place after snoozing, RO-11 a snoozed conversation's mark, named
  "Snoozed until {when}": "{when} तक टाला गया",
  // RO-14 (with Undo) in a row's place after marking it done
  "Marked as done": "पूरा चिह्नित किया गया",
  // RO-33: a time tomorrow, inside the lines above and RO-05
  "tomorrow {time}": "कल {time}",
} as const;
