// The Hindi of words-steps-live.ts, keyed by its English. Loaded only for a Hindi display: words-hi.ts gathers every words
// file's Hindi into one lazy chunk, and words-steps-live.ts imports this file's type alone.

export const HI = {
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

/** Every key, for the banned-words screen in the tests. */
export const stepLiveWords: Readonly<Record<keyof typeof HI, string>> = HI;
