// What Clauseo is doing now, one sentence for the step in hand: the working line's words while a step runs, and the
// live step group's line. Small, since the working line is on the first screen; the words of a settled group are
// words-steps.ts's, in the thread's chunk. As in words.ts the English is the key and Hindi the value; {name} is a
// paper's file name as filed, {query} the words searched for, as the agent gave them. The Hindi is a specialist's
// draft, waiting for a Hindi-speaking lawyer: the formal आप, the danda, Western digits, English names in Latin script.
// Nothing here says I or मैं, step, terminal or command, चरण or कदम.
import { wordsOf } from "./words-core.ts";

const HI = {
  "Reading {name}.": "{name} पढ़ा जा रहा है।",
  "Reading the papers.": "दस्तावेज़ पढ़े जा रहे हैं।",
  "Changing {name}.": "{name} बदला जा रहा है।",
  "Changing a paper.": "एक दस्तावेज़ बदला जा रहा है।",
  "Deleting {name}.": "{name} हटाया जा रहा है।",
  "Deleting a paper.": "एक दस्तावेज़ हटाया जा रहा है।",
  "Moving {name}.": "{name} की जगह बदली जा रही है।",
  "Moving a paper.": "एक दस्तावेज़ की जगह बदली जा रही है।",
  "Searching for '{query}'.": "'{query}' खोजा जा रहा है।",
  "Searching the papers.": "दस्तावेज़ों में खोजा जा रहा है।",
  "Searching online for '{query}'.": "ऑनलाइन '{query}' खोजा जा रहा है।",
  "Reading {name} online.": "{name} ऑनलाइन पढ़ा जा रहा है।",
  "Looking online.": "ऑनलाइन देखा जा रहा है।",
  "Running a task.": "एक काम चल रहा है।",
  "Thinking it through.": "विचार किया जा रहा है।",
  "Working on it.": "काम चल रहा है।",
} as const;

/** Every sentence of the step in hand, in English. */
export type StepLiveKey = keyof typeof HI;

/** A sentence of the step in hand, in the language, its slots filled. */
export const stepLive = wordsOf<StepLiveKey>(HI);

/** Every key, for the banned-words screen in the tests. */
export const stepLiveWords: Readonly<Record<StepLiveKey, string>> = HI;
