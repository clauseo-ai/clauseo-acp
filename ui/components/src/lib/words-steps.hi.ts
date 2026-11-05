// The Hindi of words-steps.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-steps.ts imports this file's type alone.

export const HI = {
  // a row, its step done
  "Read {name}.": "{name} पढ़ा गया।",
  "Read the papers.": "दस्तावेज़ पढ़े गए।",
  "Changed {name}.": "{name} बदला गया।",
  "Changed a paper.": "एक दस्तावेज़ बदला गया।",
  "Deleted {name}.": "{name} हटाया गया।",
  "Deleted a paper.": "एक दस्तावेज़ हटाया गया।",
  "Moved {name}.": "{name} की जगह बदली गई।",
  "Moved a paper.": "एक दस्तावेज़ की जगह बदली गई।",
  "Searched for '{query}'.": "'{query}' खोजा गया।",
  "Searched the papers.": "दस्तावेज़ों में खोजा गया।",
  "Searched online for '{query}'.": "ऑनलाइन '{query}' खोजा गया।",
  "Read {name} online.": "{name} ऑनलाइन पढ़ा गया।",
  "Looked online.": "ऑनलाइन देखा गया।",
  "Ran a task.": "एक काम चलाया गया।",
  "Thought it through.": "विचार किया गया।",
  "Did one other thing.": "एक अन्य काम किया गया।",
  // a step that failed or was stopped: what it set out to do, in the past, never as if it were still going; the row's
  // mark (Failed, Stopped) says how it ended
  "Tried to read {name}.": "{name} पढ़ने की कोशिश की गई।",
  "Tried to read the papers.": "दस्तावेज़ पढ़ने की कोशिश की गई।",
  "Tried to change {name}.": "{name} बदलने की कोशिश की गई।",
  "Tried to change a paper.": "एक दस्तावेज़ बदलने की कोशिश की गई।",
  "Tried to delete {name}.": "{name} हटाने की कोशिश की गई।",
  "Tried to delete a paper.": "एक दस्तावेज़ हटाने की कोशिश की गई।",
  "Tried to move {name}.": "{name} की जगह बदलने की कोशिश की गई।",
  "Tried to move a paper.": "एक दस्तावेज़ की जगह बदलने की कोशिश की गई।",
  "Tried to search for '{query}'.": "'{query}' खोजने की कोशिश की गई।",
  "Tried to search the papers.": "दस्तावेज़ों में खोजने की कोशिश की गई।",
  "Tried to search online for '{query}'.": "ऑनलाइन '{query}' खोजने की कोशिश की गई।",
  "Tried to read {name} online.": "{name} ऑनलाइन पढ़ने की कोशिश की गई।",
  "Tried to look online.": "ऑनलाइन देखने की कोशिश की गई।",
  "Tried to run a task.": "एक काम चलाने की कोशिश की गई।",
  "Tried to think it through.": "विचार करने की कोशिश की गई।",
  "Tried one other thing.": "एक अन्य काम करने की कोशिश की गई।",
  // a step named only by the agent's own words (its title or a task's description), tried and not done: those words
  // after the attempt, so the agent's imperative is never told as done; the words stay as the agent sent them
  "Tried: {title}.": "कोशिश की गई: {title}।",
  // a row's state, after its words, while it waits or once it carried on behind a steer
  Waiting: "प्रतीक्षा में",
  // a step whose state this version can't read (a newer Clauseo's): neither done nor failed, said as such
  "State not known": "स्थिति ज्ञात नहीं",
  // the folded line of a group whose every step is in such a state, and the row of one nothing names: no tense
  "State not known.": "स्थिति ज्ञात नहीं।",
  // the folded line: one sentence for each kind of work, the most done first
  "1 paper read.": "1 दस्तावेज़ पढ़ा गया।",
  "{n} papers read.": "{n} दस्तावेज़ पढ़े गए।",
  "1 paper changed.": "1 दस्तावेज़ बदला गया।",
  "{n} papers changed.": "{n} दस्तावेज़ बदले गए।",
  "1 paper deleted.": "1 दस्तावेज़ हटाया गया।",
  "{n} papers deleted.": "{n} दस्तावेज़ हटाए गए।",
  "1 paper moved.": "1 दस्तावेज़ की जगह बदली गई।",
  "{n} papers moved.": "{n} दस्तावेज़ों की जगह बदली गई।",
  "Searched once.": "एक बार खोजा गया।",
  "Searched {n} times.": "{n} बार खोजा गया।",
  "Looked online once.": "एक बार ऑनलाइन देखा गया।",
  "Looked online {n} times.": "{n} बार ऑनलाइन देखा गया।",
  "Ran 1 task.": "1 काम चलाया गया।",
  "Ran {n} tasks.": "{n} काम चलाए गए।",
  "Planned the work.": "काम की योजना बनाई गई।",
  "Did 1 other thing.": "1 अन्य काम किया गया।",
  "Did {n} other things.": "{n} अन्य काम किए गए।",
  // past the three kinds done most
  "And 1 more.": "और 1 काम।",
  "And {n} more.": "और {n} काम।",
  // what failed or was stopped stays on the folded line, counted on its own after the steps that finished, never as
  // "of them" (the summary counts only what finished, so "of them" would point at nothing it named); a group with
  // nothing finished says what it tried, once
  "1 failed.": "1 विफल रहा।",
  "{n} failed.": "{n} विफल रहे।",
  "1 stopped.": "1 रोका गया।",
  "{n} stopped.": "{n} रोके गए।",
  // a step a steer moved to the background stays told on the folded line (its row says "In the background", U7's).
  // Hindi in the cut the failed and stopped lines had, and in U7's own words for it ("वह काम पृष्ठभूमि में चलता रहा")
  "It went on in the background.": "यह पृष्ठभूमि में चलता रहा।",
  "1 of them went on in the background.": "उनमें से 1 पृष्ठभूमि में चलता रहा।",
  "{n} of them went on in the background.": "उनमें से {n} पृष्ठभूमि में चलते रहे।",
  // the live line, after what is being done now
  "1 done so far.": "अब तक 1 पूरा हुआ।",
  "{n} done so far.": "अब तक {n} पूरे हुए।",
} as const;

/** Every key, for the banned-words screen in the tests. */
export const stepWords: Readonly<Record<keyof typeof HI, string>> = HI;
