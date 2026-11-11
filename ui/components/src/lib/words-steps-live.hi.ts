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
  "Running {program} on {name}.": "{name} पर {program} चल रहा है।",
  "Running {program}.": "{program} चल रहा है।",
  "Searching {site} for '{query}'.": "{site} पर '{query}' खोजा जा रहा है।",
  "Thinking it through.": "विचार किया जा रहा है।",
  "Working on it.": "काम चल रहा है।",
  // a step named only by the sandbox's permission for the internet (UC-168; provisional)
  "Letting a task reach the internet.": "एक काम को इंटरनेट तक पहुँचने दिया जा रहा है।",
  // a task named by its own title or the text it shows, where it gave no other words (UC-240; provisional)
  "Running {description}.": "{description} चल रहा है।",
  // a skill read, as a guide (UC-241 to UC-245; provisional): Clauseo's own by name, any other as a guide; the
  // guides' names fill "Reading {name}." here and "Read {name}." in words-steps.ts
  "Reading a guide.": "एक मार्गदर्शिका पढ़ी जा रही है।",
  "Clauseo's drafting guide": "Clauseo की मसौदा मार्गदर्शिका",
  "Clauseo's research guide": "Clauseo की शोध मार्गदर्शिका",
  "Clauseo's matter brief guide": "Clauseo की मामला सारांश मार्गदर्शिका",
  "Clauseo's browser guide": "Clauseo की ब्राउज़र मार्गदर्शिका",
} as const;

/** Every key, for the banned-words screen in the tests. */
export const stepLiveWords: Readonly<Record<keyof typeof HI, string>> = HI;
