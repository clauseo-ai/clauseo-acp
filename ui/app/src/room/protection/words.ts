// UC: the protection command's lines, as the note, the firm's notice, the first run's step and Settings say them: the
// waiting line while a command runs ("Choose Yes in the window Windows shows." behind Windows' prompt, "Setting up the
// Safe workspace." without; R17 PR-B's in_progress keeps it after a reload) and a refusal in its code's words.
import type { Lang } from "@clauseo/components/lib/display.ts";
import { protectionErrorWords, pw } from "@clauseo/components/lib/words-protection.ts";
import type { ProtectionAct, ProtectionState } from "../../store/protection.ts";
import type { Cause } from "./causes.ts";
import { asksAdmin, inProgress } from "./view.ts";

/** The command running now, from this window or (after a reload) as the host says; null when none. */
export function busyOf(state: ProtectionState): ProtectionAct | "check" | null {
  if (state.running !== null) return state.running;
  const doing = inProgress(state.view);
  return doing;
}

/** The waiting line while a protection command runs; null when none does. */
export function waitingWords(
  lang: Lang,
  state: ProtectionState,
  cause: Cause | null,
): string | null {
  const busy = busyOf(state);
  if (busy === null) return null;
  const admin = busy === "remove" || (busy === "set_up" && asksAdmin(state.view, cause));
  return admin
    ? pw(lang, "Choose Yes in the window Windows shows.")
    : pw(lang, "Setting up the Safe workspace.");
}

/** The last refusal in words; nothing for none. */
export const failedWords = (lang: Lang, state: ProtectionState): string | null =>
  state.failed === null ? null : protectionErrorWords(lang, state.failed);
