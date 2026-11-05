// The words of the first run (UD1): the first screen, the agent getting ready at its foot and in the composer, and
// signing in where the composer's question will go. As in words.ts the English is the key, the exact string on screen,
// and a key with slots fills them in either language. The strings from the approved design
// (design/03-system/src/lib/words-first-launch.ts, interface strings only, never its sample matter's) keep the
// design's Hindi. The rest are new, drafted in the design's voice in English (QU.8): hi: later. Three of the design's
// sentences are the setup codes' own (agent.install_offline, agent.install_blocked, signin.cancelled), so the screens
// say them through words-errors.ts and they are not repeated here. Nothing names a model, and nothing says "agent".
import { useDisplay } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import type { HI } from "./words-first-launch.hi.ts";

/** Every string of the first run, in English. */
export type FirstLaunchKey = keyof typeof HI;
/** A first-run string in the language, its slots filled. */
export const fl = wordsOf("first-launch");

/** The first run's words in the page's language: const w = useFirstLaunchWords(); w("Sign in"). */
export function useFirstLaunchWords() {
  const { lang } = useDisplay();
  return (key: FirstLaunchKey, slots?: Slots) => fl(lang, key, slots);
}
