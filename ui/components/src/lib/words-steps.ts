// The words of a settled step group, in the thread's chunk: each row once its step is done, the folded line's summary
// ("4 papers read. Searched 2 times."), counted one and many apart in both languages, and the line's live count. What a
// running step is doing is words-steps-live.ts's. As in words.ts the English is the key and Hindi the value; {name} is
// a paper's file name as filed, {query} the words searched for, as the agent gave them. The Hindi is a specialist's
// draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script.
// Nothing here says I or मैं, step, terminal or command, चरण or कदम.
import { wordsOf } from "./words-core.ts";

const HI = {
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
  // a row's state, after its words, while it waits or once it carried on behind a steer
  Waiting: "प्रतीक्षा में",
  "In the background": "पृष्ठभूमि में",
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
  // what failed or was stopped stays on the folded line: a group of one, or one of many
  "It failed.": "यह विफल रहा।",
  "1 of them failed.": "उनमें से 1 विफल रहा।",
  "{n} of them failed.": "उनमें से {n} विफल रहे।",
  "It was stopped.": "इसे रोका गया।",
  "1 of them was stopped.": "उनमें से 1 रोका गया।",
  "{n} of them were stopped.": "उनमें से {n} रोके गए।",
  // the live line, after what is being done now
  "1 done so far.": "अब तक 1 पूरा हुआ।",
  "{n} done so far.": "अब तक {n} पूरे हुए।",
} as const;

/** Every string of a settled step group, in English. */
export type StepKey = keyof typeof HI;

/** A string of a step group, in the language, its slots filled. */
export const steps = wordsOf<StepKey>(HI);

/** Every key, for the banned-words screen in the tests. */
export const stepWords: Readonly<Record<StepKey, string>> = HI;
