// UD1's welcome counts line with crash reports in it (0060, UC-750), apart from registry-ud1.tsx to keep that file under
// the size limit. It takes the place of UD1-08, whose null slot registry-ud1.tsx keeps.
import "@clauseo/components/lib/words-hi.ts";
import { fl, type FirstLaunchKey } from "@clauseo/components/lib/words-first-launch.ts";
import type { Entry } from "./entry.ts";
import { WELCOME } from "./registry-ud1.tsx";
import { render } from "./registry-ud1-scene.tsx";
import type { WordsRef } from "./words.ts";

const firstLaunchWords = (key: FirstLaunchKey): WordsRef => ({
  file: "lib/words-first-launch.ts",
  key,
  text: (lang) => fl(lang, key),
  later: fl.hi[key] === null,
});

/** The welcome's counts line, on the first screen. */
const COUNTS_LINE: Entry = {
  id: "UC-750",
  words: firstLaunchWords(
    "How many questions, which features, how long answers take, and a short report if Clauseo crashes. Never your papers, your questions or the answers.",
  ),
  screen: "first-launch/welcome",
  render: render(WELCOME),
};

export const UD1_CRASH: readonly Entry[] = [COUNTS_LINE];
