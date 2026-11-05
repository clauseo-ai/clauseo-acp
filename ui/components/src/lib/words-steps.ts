// The words of a settled step group, in the thread's chunk: each row once its step is done, the folded line's summary
// ("4 papers read. Searched 2 times."), counted one and many apart in both languages, and the line's live count. What a
// running step is doing is words-steps-live.ts's. As in words.ts the English is the key and Hindi the value; {name} is
// a paper's file name as filed, {query} the words searched for, as the agent gave them. The Hindi is a specialist's
// draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script.
// Nothing here says I or मैं, step, terminal or command, चरण or कदम.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-steps.hi.ts";

/** Every string of a settled step group, in English. */
export type StepKey = keyof typeof HI;

/** A string of a step group, in the language, its slots filled. */
export const steps = wordsOf("steps");
