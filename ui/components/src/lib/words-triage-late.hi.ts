// The Hindi of words-triage-late.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-triage-late.ts imports this file's type alone.

export const HI = {
  // RO-07: the menu's heading, and the row of a matter's menu that opens it (RO-15)
  "Snooze until": "कब तक टालें",
  // RO-08: the menu's times, and the picker
  "Tomorrow {time}": "कल {time}",
  "Monday {time}": "सोमवार {time}",
  "Choose a day and time": "दिन और समय चुनें",
  // RO-09: with "{weekday} {d} {month}, {time}" beneath, 9:00 am the day before the matter's next date
  "Before the next date": "अगली तारीख से पहले",
  // RO-05: a snoozed row's line in the shelf; {when} as in "Next back {when}"
  "Until {when}": "{when} तक",
  // RO-06: a matter snoozed whole, in the shelf
  "Whole matter": "पूरा मामला",
  // RO-12: a shelf row's button
  "Wake now": "अभी वापस लाएँ",
  // RO-04: behind the open shelf's info icon
  "Back sooner if Clauseo needs you or an answer finishes.":
    "Clauseo को आपकी ज़रूरत होने या कोई उत्तर पूरा होने पर पहले लौट आता है।",
  // RO-13: a conversation's two buttons and its menu's items, and a done conversation's button
  Snooze: "टालें",
  "Mark as done": "पूरा चिह्नित करें",
  "Mark as not done": "अधूरा चिह्नित करें",
  // RO-15: a matter's menu, with RO-07's "Snooze until" on its submenu's row
  "Mark all done": "सभी को पूरा चिह्नित करें",
  // RO-17: behind the open fold's info icon
  "Conversations you've read, with nothing waiting, move here each morning.":
    "जो बातचीत आप पढ़ चुके हैं और जिनमें कुछ बाकी नहीं, वे हर सुबह यहाँ आ जाती हैं।",
} as const;
