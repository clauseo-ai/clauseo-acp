// The Hindi of words-frame.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-frame.ts imports this file's type alone.

export const HI = {
  // the title bar on Windows: the caption buttons and their group; Restore and Close are words-ui.ts's
  Window: "विंडो",
  Minimise: "न्यूनतम करें",
  Maximise: "अधिकतम करें",
  // the matter list: what each matter is doing; and the composer working, with no end time: the app never guesses
  // when an answer will end
  "Answering.": "उत्तर दिया जा रहा है।",
  "Answering. Expected by {time}": "उत्तर तैयार हो रहा है। {time} बजे तक अपेक्षित",
  "A question for you": "आपसे एक प्रश्न",
  "Needs your permission": "आपकी अनुमति चाहिए",
  "Answer ready": "उत्तर तैयार है",
  "Stopped at {time}": "{time} बजे रोका गया",
  "Stopped on {day} at {time}": "{day} को {time} बजे रोका गया",
  // U16a: a failed answer, until its conversation is seen (hi: later)
  "Failed at {time}": null,
  "Failed on {day} at {time}": null,
  // paused at the plan's limit, until the reset; the usage limit
  "Paused until {time}": "{time} बजे तक रुका है",
  "Paused until {day} {time}": "{day} {time} बजे तक रुका है",
  "Paused at the plan's limit": "प्लान की सीमा पर रुका है",
  "Limit reached. Continue at {time}?": "सीमा पूरी। {time} बजे जारी\u00a0रखें?",
  "Limit reached. Continue on {day} at {time}?": "सीमा पूरी। {day} को {time} बजे जारी\u00a0रखें?",
  Pinned: "पिन किए गए",
  // U16a: the group of matters waiting on the lawyer, under Pinned; Recent's day headings (hi: later)
  "Needs you": null,
  Today: null,
  Yesterday: null,
  // the date groups, by each matter's next date, in this order
  Tomorrow: "कल",
  "This week": "इस सप्ताह",
  "Next week": "अगले सप्ताह",
  Later: "बाद में",
  "No date yet": "अभी कोई तारीख नहीं",
  "No matters yet": "अभी कोई मामला नहीं",
  "Add a matter's folder.": "मामले का फ़ोल्डर जोड़ें।",
  "Add a matter's folder. Clauseo reads its papers before your first question.":
    "मामले का फ़ोल्डर जोड़ें। आपके पहले प्रश्न से पहले Clauseo उसके दस्तावेज़ पढ़ लेता है।",
  // settings: how Clauseo looks
  Theme: "थीम",
  "Follow the computer": "कंप्यूटर के अनुसार",
  "Text size": "अक्षरों का आकार",
  "Every word on screen grows or shrinks with it.": "स्क्रीन के सभी शब्द इसके साथ बड़े या छोटे होते हैं।",
  "Reduced motion": "कम हलचल",
  "Every change lands at once, and nothing moves.": "हर बदलाव तुरंत दिखता है और कुछ भी हिलता नहीं।",
} as const;
