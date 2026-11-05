// What Clauseo is doing now, one sentence for the step in hand: the working line's words while a step runs, and the
// live step group's line. Small, since the working line is on the first screen; the words of a settled group are
// words-steps.ts's, in the thread's chunk. As in words.ts the English is the key and Hindi the value; {name} is a
// paper's file name as filed, {query} the words searched for, as the agent gave them. The Hindi is a specialist's
// draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script.
// Nothing here says I or मैं, step, terminal or command, चरण or कदम.
import { wordsOf } from "./words-core.ts";
import type { HI } from "./words-steps-live.hi.ts";

/** Every sentence of the step in hand, in English. */
export type StepLiveKey = keyof typeof HI;

/** A sentence of the step in hand, in the language, its slots filled. */
export const stepLive = wordsOf("steps-live");
