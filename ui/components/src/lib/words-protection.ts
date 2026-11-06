// The words of protection (UC) the room and the settings window load with their protection parts, never in a first
// load: the set-up and remove confirms, the notes' bodies the shipped ones don't cover, the pool's counts and its ways
// out, the firm's required notice, and the pool's refusal. As in words.ts the English is the key, the exact string on
// screen. All are new, drafted in DESIGN's voice ("protection" and "Safe workspace", never sandbox, agent or session),
// provisional for Rohan's batch review; their Hindi comes later (hi: later). The room's line (words-toolbar.ts), the
// first run's (words-first-launch.ts), the shipped note words (words-versions.ts) and Settings' (words-settings.ts)
// live with their screens.
import type { Lang } from "./display.ts";
import { wordsOf, type Slots } from "./words-core.ts";
import { errorWords } from "./words-errors.ts";
import type { HI } from "./words-protection.hi.ts";

/** Every protection string, in English. */
export type ProtectionKey = keyof typeof HI;

/** A protection string in the language, its slots filled. */
export const pw = wordsOf("protection");

/** R17 PR-E's code for a full pool under `sandbox.required`; not in ErrorCode yet, so its words live here. */
export const POOL_FULL_CODE = "sandbox.pool_full";

/** A protection command's or launch's refusal in words: the pool's own code here, every other through errorWords. */
export function protectionErrorWords(lang: Lang, code: string): string {
  return code === POOL_FULL_CODE
    ? pw(
        lang,
        "Your firm requires protection, and all of this computer's protected spaces are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.",
      )
    : errorWords(lang, code);
}

/** The protection words of a language, bound. */
export const protectionWords = (lang: Lang) => (key: ProtectionKey, slots?: Slots) =>
  pw(lang, key, slots);
