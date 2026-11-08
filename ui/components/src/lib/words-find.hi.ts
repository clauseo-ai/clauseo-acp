// The Hindi of words-find.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-find.ts imports this file's type alone.

export const HI = {
  // RO-21: the second heading, and a conversation row's line: the matter, then the day ("today", or clock's day)
  Conversations: "बातचीत",
  "{matter}, {day}": "{matter}, {day}",
  today: "आज",
  // RO-22: what Find covers, behind the info icon at the results' foot
  "Searches titles, paper names and conversations.": "शीर्षकों, दस्तावेज़ों के नामों और बातचीत में खोजता है।",
  // RO-24: whose words a content match is
  "You:": "आप:",
  "Clauseo:": "Clauseo:",
  // RO-25: a paper found by its file name, as its icon's accessible name
  "Paper, {file}": "दस्तावेज़, {file}",
  // RO-26: nothing found
  'Nothing found for "{words}".': '"{words}" के लिए कुछ नहीं मिला।',
  // RO-27: while the index is still being built
  "Still adding older conversations.": "पुरानी बातचीत अभी जोड़ी जा रही हैं।",
  // RO-32: what that means, behind the info icon at its end
  "Some may not show yet.": "कुछ अभी शायद न दिखें।",
  // RO-28: an archived conversation's icon, named
  Archived: "संग्रहीत",
} as const;
