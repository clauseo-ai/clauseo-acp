// The words of Find's results (U16a): what a search inside the matters found, said on the rail under the field. Loaded
// with the results (room/find-results.tsx), never on first paint; the field's own word, "Find", stays in
// words-matters.ts. As in words.ts the English is the key. Every line here is new and its Hindi is still to come (null).
import { wordsOf } from "./words-core.ts";

const HI = {
  // RO-21: the second heading, and a conversation row's line: the matter, then the day ("today", or clock's day)
  Conversations: null,
  "{matter}, {day}": null,
  today: null,
  // RO-22: what Find covers, behind the info icon at the results' foot
  "Searches titles, paper names and conversations.": null,
  // RO-24: whose words a content match is
  "You:": null,
  "Clauseo:": null,
  // RO-25: a paper found by its file name, as its icon's accessible name
  "Paper, {file}": null,
  // RO-26: nothing found
  'Nothing found for "{words}".': null,
  // RO-27: while the index is still being built
  "Still adding older conversations.": null,
  // RO-32: what that means, behind the info icon at its end
  "Some may not show yet.": null,
  // RO-28: an archived conversation's icon, named
  Archived: null,
} as const;

export type FindKey = keyof typeof HI;

/** Find's results' words in `lang`, their slots filled. */
export const fw = wordsOf<FindKey>(HI);
