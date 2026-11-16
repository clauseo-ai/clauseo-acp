// The words of protection (UC) the room and the settings window load with their protection parts,
// never in a first load: the Add room and remove confirms, what stops the Safe workspace and the
// repair (design B), the notes' bodies the shipped ones don't cover, the pool's counts and its ways
// out, the firm's required notice, and the pool's refusal. As in words.ts the English is the key,
// the exact string on screen. All are new, drafted in DESIGN's voice ("Safe workspace", never
// sandbox, agent or session), provisional for Rohan's batch review; the Safe workspace lines carry
// provisional Hindi, the rest come later. The room's line (words-toolbar.ts), the first run's
// (words-first-launch.ts), the shipped note words (words-versions.ts) and Settings'
// (words-settings.ts) live with their screens.
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

/** #639's code for a Safe workspace something on this computer stops; not in ErrorCode on this
 * base. Its words are the generic ones: the reason's own line comes from the protection state's
 * cause (blocked.ts), never the turn. */
export const BLOCKED_CODE = "sandbox.blocked";

/** The line beside "Get the installer" on a personal install: the button quits Clauseo once the
 * page is open (the app's quit, desktop's quit_app), so the lawyer knows before clicking. */
export const INSTALLER_CLOSE: ProtectionKey =
  "Clauseo will close so the installer can set up the Safe workspace. Run the installer you download, then open Clauseo again.";

/** A protection command's or launch's refusal in words: the pool's and the block's codes here,
 * every other through errorWords. */
export function protectionErrorWords(lang: Lang, code: string): string {
  if (code === BLOCKED_CODE)
    return pw(
      lang,
      "Something on this computer stops the Safe workspace, so Clauseo can't answer here. Settings › Safe workspace says what to do.",
    );
  return code === POOL_FULL_CODE
    ? pw(
        lang,
        "Your firm requires the Safe workspace, and all its places on this computer are in use, so Clauseo can't answer here yet. End a conversation in another matter to free one.",
      )
    : errorWords(lang, code);
}

/** The protection words of a language, bound. */
export const protectionWords = (lang: Lang) => (key: ProtectionKey, slots?: Slots) =>
  pw(lang, key, slots);
